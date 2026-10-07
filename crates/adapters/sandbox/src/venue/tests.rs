use model::{
    CancelOrder, ClientOrderId, Decimal, InstrumentId, InstrumentInfo, OrderEvent, PriceBand,
    Quote, Side, SubmitOrder, TickTable, UnixNanos,
};

use super::listing::{walk_ticks, PULL_RANGE};
use super::*;

const NOW: UnixNanos = UnixNanos(1);

fn d(raw: &str) -> Decimal {
    raw.parse().expect("test decimal")
}

fn aaa() -> InstrumentId {
    InstrumentId::new("sim", "AAA")
}

fn venue(cash: &str, mid: &str) -> SimVenue {
    let venue = SimVenue::new(d(cash));
    venue.list(aaa(), d(mid));
    venue
}

fn order(id: u64, side: Side, quantity: &str, limit: Option<&str>) -> SubmitOrder {
    SubmitOrder {
        order_id: ClientOrderId(id),
        instrument: aaa(),
        side,
        quantity: d(quantity),
        limit_price: limit.map(d),
    }
}

fn submit(venue: &SimVenue, order: SubmitOrder) -> Vec<Event> {
    let mut out = Vec::new();
    venue.submit(order, NOW, &mut |e| out.push(e));
    out
}

fn step(venue: &SimVenue, delta: &str) -> (Quote, Vec<Event>) {
    let mut out = Vec::new();
    let tick = venue
        .step(&aaa(), d(delta), NOW, &mut |e| out.push(e))
        .expect("listed");
    (tick.quote, out)
}

fn cancel(venue: &SimVenue, request: &CancelOrder) -> Vec<Event> {
    let mut out = Vec::new();
    venue.cancel(request, NOW, &mut |e| out.push(e));
    out
}

fn order_events(events: &[Event]) -> Vec<OrderEvent> {
    events
        .iter()
        .filter_map(|e| match e {
            Event::Execution(r) => Some(r.event.clone()),
            _ => None,
        })
        .collect()
}

fn fills(events: &[Event]) -> Vec<(Decimal, Decimal)> {
    order_events(events)
        .into_iter()
        .filter_map(|e| match e {
            OrderEvent::Filled {
                quantity, price, ..
            } => Some((quantity, price)),
            _ => None,
        })
        .collect()
}

#[test]
fn market_buy_is_accepted_then_filled_at_the_ask() {
    let venue = venue("1000", "10");

    let events = submit(&venue, order(1, Side::Buy, "5", None));

    assert!(matches!(
        order_events(&events)[0],
        OrderEvent::Accepted { .. }
    ));
    assert_eq!(fills(&events), vec![(d("5"), d("10.01"))]);
    let account = venue.account();
    assert_eq!(account.cash, d("1000") - d("5") * d("10.01"));
    assert_eq!(account.positions[0].quantity, d("5"));
}

#[test]
fn buy_beyond_available_cash_is_rejected_without_touching_the_account() {
    let venue = venue("10", "10");

    let events = submit(&venue, order(1, Side::Buy, "5", None));

    assert!(matches!(
        order_events(&events)[..],
        [OrderEvent::Rejected { .. }]
    ));
    assert_eq!(venue.account().cash, d("10"));
}

#[test]
fn resting_buys_reserve_cash_against_later_orders() {
    let venue = venue("1000", "10");
    submit(&venue, order(1, Side::Buy, "90", Some("9")));

    let events = submit(&venue, order(2, Side::Buy, "30", Some("9")));

    assert!(matches!(
        order_events(&events)[..],
        [OrderEvent::Rejected { .. }]
    ));
}

#[test]
fn unlisted_instrument_is_rejected() {
    let venue = venue("1000", "10");
    let mut unknown = order(1, Side::Buy, "1", None);
    unknown.instrument = InstrumentId::new("sim", "ZZZ");

    let events = submit(&venue, unknown);

    assert!(matches!(
        order_events(&events)[..],
        [OrderEvent::Rejected { .. }]
    ));
}

#[test]
fn non_marketable_limit_rests_until_the_price_reaches_it() {
    let venue = venue("1000", "10");

    let submitted = submit(&venue, order(1, Side::Buy, "1", Some("9.95")));
    let (_, first_move) = step(&venue, "-0.03");
    let (_, second_move) = step(&venue, "-0.03");

    assert!(fills(&submitted).is_empty());
    assert!(fills(&first_move).is_empty());
    assert_eq!(fills(&second_move), vec![(d("1"), d("9.95"))]);
}

#[test]
fn a_large_resting_order_fills_across_several_price_moves() {
    let venue = venue("100000", "10");
    submit(&venue, order(1, Side::Buy, "250", Some("9.95")));

    let filled: Vec<Decimal> = ["-0.06", "0", "0", "0"]
        .into_iter()
        .flat_map(|delta| {
            let (_, events) = step(&venue, delta);
            fills(&events).into_iter().map(|(q, _)| q)
        })
        .collect();

    assert_eq!(filled, vec![d("100"), d("100"), d("50")]);
}

#[test]
fn marketable_limit_takes_only_the_displayed_size_now() {
    let venue = venue("100000", "10");

    let events = submit(&venue, order(1, Side::Buy, "150", Some("10.05")));

    assert_eq!(fills(&events), vec![(d("100"), d("10.01"))]);
}

#[test]
fn cancel_removes_a_resting_order() {
    let venue = venue("1000", "10");
    submit(&venue, order(1, Side::Buy, "1", Some("9")));
    let request = CancelOrder {
        order_id: ClientOrderId(1),
        venue_order_id: None,
        instrument: aaa(),
    };

    let first = cancel(&venue, &request);
    let second = cancel(&venue, &request);

    assert_eq!(order_events(&first), vec![OrderEvent::Cancelled]);
    assert!(matches!(
        order_events(&second)[..],
        [OrderEvent::CancelRejected { .. }]
    ));
}

#[test]
fn average_price_blends_when_adding_to_a_position() {
    let venue = venue("10000", "10");
    submit(&venue, order(1, Side::Buy, "1", None));
    step(&venue, "10");
    submit(&venue, order(2, Side::Buy, "1", None));

    let position = &venue.account().positions[0];

    assert_eq!(position.quantity, d("2"));
    assert_eq!(position.avg_price, d("15.01"));
}

#[test]
fn selling_through_zero_starts_a_short_at_the_fill_price() {
    let venue = venue("10000", "10");
    submit(&venue, order(1, Side::Buy, "5", None));

    submit(&venue, order(2, Side::Sell, "8", None));

    let position = &venue.account().positions[0];
    assert_eq!(position.quantity, d("-3"));
    assert_eq!(position.avg_price, d("9.99"));
}

#[test]
fn closing_a_position_removes_it() {
    let venue = venue("10000", "10");
    submit(&venue, order(1, Side::Buy, "5", None));

    submit(&venue, order(2, Side::Sell, "5", None));

    assert!(venue.account().positions.is_empty());
}

#[test]
fn synthetic_history_ends_at_the_current_mid_without_moving_it() {
    let venue = venue("1000", "10");
    let spacing = Duration::from_millis(500);
    let end = UnixNanos(1_000_000_000_000);

    let history = venue.synthetic_history(&aaa(), end, 50, spacing, &mut 7);

    assert_eq!(history.len(), 50);
    assert_eq!(history.last().map(|q| q.mid()), Some(d("10")));
    assert_eq!(
        history.last().map(|q| q.ts_event),
        Some(UnixNanos(end.0 - 500_000_000))
    );
    assert!(history
        .windows(2)
        .all(|w| w[1].ts_event.0 - w[0].ts_event.0 == 500_000_000));
    assert!(history.iter().all(|q| q.mid() >= MIN_MID));
    assert_eq!(venue.quote(&aaa(), NOW).map(|q| q.mid()), Some(d("10")));
}

#[test]
fn synthetic_history_is_reproducible_from_its_seed() {
    let venue = venue("1000", "10");
    let spacing = Duration::from_millis(500);
    let end = UnixNanos(10_000_000_000);

    let first = venue.synthetic_history(&aaa(), end, 20, spacing, &mut 42);
    let second = venue.synthetic_history(&aaa(), end, 20, spacing, &mut 42);

    assert_eq!(first, second);
}

#[test]
fn synthetic_history_of_an_unlisted_instrument_is_empty() {
    let venue = venue("1000", "10");

    let history = venue.synthetic_history(
        &InstrumentId::new("sim", "ZZZ"),
        NOW,
        10,
        Duration::from_millis(500),
        &mut 7,
    );

    assert!(history.is_empty());
}

#[test]
fn the_bid_never_walks_below_one_tick() {
    let venue = venue("1000", "0.05");

    let (quote, _) = step(&venue, "-5");

    assert_eq!(quote.bid, TICK);
    assert_eq!(quote.mid(), MIN_MID);
}

#[test]
fn an_order_whose_value_overflows_is_rejected_not_panicked() {
    let venue = venue("1000", "10");

    for side in [Side::Buy, Side::Sell] {
        let mut absurd = order(1, side, "2", None);
        absurd.limit_price = Some(Decimal::MAX);

        let events = submit(&venue, absurd);

        assert!(
            matches!(order_events(&events)[..], [OrderEvent::Rejected { .. }]),
            "{side:?}"
        );
    }
    assert_eq!(venue.account().cash, d("1000"));
}

/// A HOSE-style listing: the 10/50/100 ladder, a 7% band, lots of 100.
fn banded(symbol: &str, reference: &str) -> Listing {
    let id = InstrumentId::new("sim", symbol);
    let ticks = TickTable::new(vec![
        (d("0"), d("10")),
        (d("10000"), d("50")),
        (d("50000"), d("100")),
    ])
    .unwrap();
    let band = PriceBand::from_fraction(id.clone(), d(reference), d("0.07"), &ticks, NOW).unwrap();
    Listing {
        info: InstrumentInfo::new(id, d("100"), ticks),
        band,
        display_size: d("10000"),
    }
}

#[test]
fn a_lean_is_a_tradable_price_inside_the_band_and_the_same_every_run() {
    for symbol in ["VNM", "FPT", "SHS", "TCB", "HPG"] {
        let listing = banded(symbol, "62500");
        let lean = listing.lean();

        assert!(listing.band.contains(lean), "{symbol} leans to {lean}");
        assert!(listing.ticks().is_on_tick(lean), "{symbol} leans to {lean}");
        assert_eq!(lean, banded(symbol, "62500").lean());
    }
}

#[test]
fn the_walk_moves_at_most_a_tick_and_usually_not_at_all() {
    let mut seed = 7;
    let steps: Vec<i64> = (0..3_000)
        .map(|_| walk_ticks(&mut seed, d("62500"), d("62500"), d("62500")))
        .collect();

    let still = steps.iter().filter(|s| **s == 0).count();
    assert!(steps.iter().all(|s| (-1..=1).contains(s)));
    assert!(
        (1_700..2_300).contains(&still),
        "{still} of 3000 beats stood still"
    );
}

#[test]
fn far_from_its_lean_every_move_heads_back() {
    let mut seed = 11;
    let moves: Vec<i64> = (0..300)
        .map(|_| walk_ticks(&mut seed, d("70000"), d("62500"), d("62500")))
        .filter(|s| *s != 0)
        .collect();

    assert!(!moves.is_empty());
    assert!(moves.iter().all(|s| *s == -1));
}

#[test]
fn the_walk_hovers_near_its_lean_and_never_leaves_the_band() {
    let listing = banded("VNM", "62500");
    let (lean, ceiling, floor) = (
        listing.lean(),
        listing.band.ceiling.unwrap(),
        listing.band.floor.unwrap(),
    );
    let venue = SimVenue::new(d("1000"));
    venue.list_with(listing);
    let instrument = InstrumentId::new("sim", "VNM");
    let mut seed = 3;

    let mids: Vec<Decimal> = (0..20_000)
        .map(|_| {
            let tick = venue
                .walk(&instrument, NOW, &mut seed, &mut |_| {})
                .expect("listed");
            assert!(tick.quote.bid >= floor && tick.quote.ask <= ceiling);
            (tick.quote.bid + tick.quote.ask) / Decimal::TWO
        })
        .collect();

    let settled = &mids[10_000..];
    let mean = settled.iter().copied().sum::<Decimal>() / Decimal::from(settled.len());
    assert!(
        (mean - lean).abs() < d("62500") * PULL_RANGE,
        "mean {mean} strays from lean {lean}"
    );
}

#[test]
fn history_stays_inside_the_band() {
    let listing = banded("SHS", "13600");
    let (ceiling, floor) = (listing.band.ceiling.unwrap(), listing.band.floor.unwrap());
    let venue = SimVenue::new(d("1000"));
    venue.list_with(listing);
    let mut seed = 5;

    let history = venue.synthetic_history(
        &InstrumentId::new("sim", "SHS"),
        UnixNanos(10_000_000_000_000),
        2_400,
        Duration::from_millis(500),
        &mut seed,
    );

    assert_eq!(history.len(), 2_400);
    assert!(history.iter().all(|q| q.bid >= floor && q.ask <= ceiling));
}

fn submit_vnm(venue: &SimVenue, quantity: &str, limit: Option<&str>) -> Vec<OrderEvent> {
    let mut out = Vec::new();
    venue.submit(
        SubmitOrder {
            order_id: ClientOrderId(1),
            instrument: InstrumentId::new("sim", "VNM"),
            side: Side::Buy,
            quantity: d(quantity),
            limit_price: limit.map(d),
        },
        NOW,
        &mut |e| out.push(e),
    );
    order_events(&out)
}

fn rejection(events: &[OrderEvent]) -> Option<String> {
    events.iter().find_map(|e| match e {
        OrderEvent::Rejected { reason } => Some(reason.clone()),
        _ => None,
    })
}

fn vnm_venue() -> SimVenue {
    let venue = SimVenue::new(d("100000000"));
    venue.list_with(banded("VNM", "62500"));
    venue
}

#[test]
fn the_venue_rejects_an_odd_lot_on_its_own() {
    let reason = rejection(&submit_vnm(&vnm_venue(), "150", Some("62500")));

    assert_eq!(
        reason.as_deref(),
        Some("quantity must be a multiple of the lot size, 100")
    );
}

#[test]
fn the_venue_rejects_a_price_between_ticks_on_its_own() {
    let reason = rejection(&submit_vnm(&vnm_venue(), "100", Some("62550")));

    assert_eq!(
        reason.as_deref(),
        Some("price must be a multiple of the tick size, 100")
    );
}

#[test]
fn the_venue_rejects_a_price_outside_the_band_on_its_own() {
    let reason = rejection(&submit_vnm(&vnm_venue(), "100", Some("66900")));

    assert_eq!(
        reason.as_deref(),
        Some("price is outside today's ceiling and floor")
    );
}

#[test]
fn a_step_down_from_a_tier_boundary_lands_on_the_price_just_below() {
    let venue = SimVenue::new(d("1000"));
    venue.list_with(banded("TCB", "10000"));
    let instrument = InstrumentId::new("sim", "TCB");

    let tick = venue
        .step_ticks(&instrument, -1, NOW, &mut |_| {})
        .expect("listed");

    assert_eq!(tick.quote.ask, d("10000"));
    assert_eq!(tick.quote.bid, d("9980"));
}
