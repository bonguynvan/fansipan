//! Vietnam's simulated trading day, through the venue's public API: which
//! order types each exchange takes in each phase, what they do, and the
//! auctions and the close that end phases.

use bus::Event;
use fansipan_sandbox::{Clock, MarketDay, MarketProfile, Phase, SimVenue, TradingDay};
use model::{
    CancelOrder, ClientOrderId, Decimal, InstrumentId, NamedOrderType, NewOrder, OrderEvent,
    OrderKind, OrderType, SessionPhase, Side, SubmitOrder, TimeInForce, UnixNanos,
};

const SECOND: u64 = 1_000_000_000;
const START: UnixNanos = UnixNanos(1_000 * SECOND);
/// The demo cycle opens 90 seconds in, on morning continuous trading.
const OPENING_SECONDS: u64 = 90;
const CYCLE_SECONDS: u64 = 900;

// Seconds into the demo cycle, which starts at 09:00.
const ATO: u64 = 0;
const MORNING: u64 = 120;
const LUNCH: u64 = 330;
const AFTERNOON: u64 = 400;
const ATC: u64 = 600;
const AFTER_CLOSE: u64 = 700;
const CLOSED: u64 = 800;

fn at(second: u64) -> UnixNanos {
    let elapsed = (second + CYCLE_SECONDS - OPENING_SECONDS) % CYCLE_SECONDS;
    UnixNanos(START.0 + elapsed * SECOND)
}

fn d(raw: &str) -> Decimal {
    raw.parse().expect("test decimal")
}

fn venue() -> SimVenue {
    MarketProfile::by_id("vn", "sim")
        .expect("vn profile")
        .venue_on(Clock::Compressed { start: START })
}

fn id(symbol: &str) -> InstrumentId {
    InstrumentId::new("sim", symbol)
}

fn typed(kind: OrderKind, time_in_force: TimeInForce) -> OrderType {
    OrderType::new(kind, time_in_force)
}

fn order(
    order_id: u64,
    symbol: &str,
    quantity: &str,
    order_type: OrderType,
    limit: Option<&str>,
) -> SubmitOrder {
    let new = NewOrder::new(id(symbol), Side::Buy, d(quantity), order_type, limit.map(d));
    SubmitOrder::new(ClientOrderId(order_id), new)
}

fn submit(venue: &SimVenue, order: SubmitOrder, now: UnixNanos) -> Vec<Event> {
    let mut out = Vec::new();
    venue.submit(order, now, &mut |e| out.push(e));
    out
}

fn advance(venue: &SimVenue, now: UnixNanos) -> Vec<Event> {
    let mut out = Vec::new();
    venue.advance(now, &mut |e| out.push(e));
    out
}

/// Each order's events, in order, as `(order id, event)`.
fn reports(events: &[Event]) -> Vec<(u64, OrderEvent)> {
    events
        .iter()
        .filter_map(|e| match e {
            Event::Execution(report) => Some((report.order_id.0, report.event.clone())),
            _ => None,
        })
        .collect()
}

/// `(quantity, price)` of each fill for `order_id`.
fn fills(events: &[Event], order_id: u64) -> Vec<(Decimal, Decimal)> {
    reports(events)
        .into_iter()
        .filter(|(id, _)| *id == order_id)
        .filter_map(|(_, event)| match event {
            OrderEvent::Filled {
                quantity, price, ..
            } => Some((quantity, price)),
            _ => None,
        })
        .collect()
}

fn ended(events: &[Event], order_id: u64) -> bool {
    reports(events)
        .iter()
        .any(|(id, event)| *id == order_id && *event == OrderEvent::Cancelled)
}

fn refusal(events: &[Event]) -> Option<String> {
    reports(events)
        .into_iter()
        .find_map(|(_, event)| match event {
            OrderEvent::Rejected { reason } => Some(reason),
            _ => None,
        })
}

fn sessions(events: &[Event]) -> Vec<(String, SessionPhase, Option<String>)> {
    events
        .iter()
        .filter_map(|e| match e {
            Event::Session(s) => Some((s.market.clone(), s.phase, s.name.clone())),
            _ => None,
        })
        .collect()
}

#[test]
fn each_market_announces_its_session_then_only_its_changes() {
    let venue = venue();

    let first = advance(&venue, at(ATO));
    let same = advance(&venue, at(ATO + 10));
    let opened = advance(&venue, at(MORNING));

    assert_eq!(
        sessions(&first),
        vec![
            (
                "HOSE".into(),
                SessionPhase::OpeningAuction,
                Some("ATO".into())
            ),
            ("HNX".into(), SessionPhase::Continuous, None),
            ("UPCOM".into(), SessionPhase::Continuous, None),
        ]
    );
    assert!(same.is_empty());
    assert_eq!(
        sessions(&opened),
        vec![("HOSE".into(), SessionPhase::Continuous, None)]
    );
}

#[test]
fn an_ato_order_waits_for_the_auction_and_fills_at_its_price() {
    let venue = venue();
    advance(&venue, at(ATO));
    let ato = typed(OrderKind::Market, TimeInForce::AtOpen);

    let queued = submit(&venue, order(1, "VNM", "100", ato, None), at(ATO + 5));
    let below = submit(
        &venue,
        order(2, "VNM", "100", OrderType::LIMIT, Some("62000")),
        at(ATO + 5),
    );
    let opened = advance(&venue, at(MORNING));

    assert!(fills(&queued, 1).is_empty() && fills(&below, 2).is_empty());
    assert_eq!(fills(&opened, 1), vec![(d("100"), d("62500"))]);
    assert!(
        fills(&opened, 2).is_empty(),
        "a limit below the auction rests on"
    );
}

#[test]
fn the_closing_auction_sets_the_price_plo_orders_fill_at() {
    let venue = venue();
    advance(&venue, at(ATC));
    let atc = typed(OrderKind::Market, TimeInForce::AtClose);
    let plo = typed(OrderKind::Market, TimeInForce::AfterClose);

    submit(&venue, order(1, "SHS", "100", atc, None), at(ATC + 5));
    let closed = advance(&venue, at(AFTER_CLOSE));
    let after = submit(
        &venue,
        order(2, "SHS", "200", plo, None),
        at(AFTER_CLOSE + 5),
    );
    let limit = submit(
        &venue,
        order(3, "SHS", "100", OrderType::LIMIT, Some("13600")),
        at(AFTER_CLOSE + 5),
    );

    assert_eq!(fills(&closed, 1), vec![(d("100"), d("13600"))]);
    assert_eq!(fills(&after, 2), vec![(d("200"), d("13600"))]);
    assert_eq!(refusal(&limit).as_deref(), Some("HNX takes only PLO now"));
}

#[test]
fn continuous_types_take_only_what_is_shown() {
    // HNX shows 10,000 shares at an ask of 13,700 for SHS.
    let venue = venue();
    let now = at(MORNING);
    advance(&venue, now);
    let mok = typed(OrderKind::Market, TimeInForce::FillOrKill);
    let mak = typed(OrderKind::Market, TimeInForce::ImmediateOrCancel);
    let mtl = typed(OrderKind::MarketToLimit, TimeInForce::Day);

    let too_big = submit(&venue, order(1, "SHS", "20000", mok, None), now);
    let whole = submit(&venue, order(2, "SHS", "5000", mok, None), now);
    let part = submit(&venue, order(3, "SHS", "15000", mak, None), now);
    let rest = submit(&venue, order(4, "SHS", "15000", mtl, None), now);
    let mut cancel = Vec::new();
    venue.cancel(
        &CancelOrder::new(ClientOrderId(4), None, id("SHS")),
        now,
        &mut |e| cancel.push(e),
    );

    assert!(fills(&too_big, 1).is_empty() && ended(&too_big, 1));
    assert_eq!(fills(&whole, 2), vec![(d("5000"), d("13700"))]);
    assert_eq!(fills(&part, 3), vec![(d("10000"), d("13700"))]);
    assert!(ended(&part, 3));
    assert_eq!(fills(&rest, 4), vec![(d("10000"), d("13700"))]);
    assert!(
        !ended(&rest, 4),
        "the rest of an MTL order rests as a limit"
    );
    assert!(ended(&cancel, 4));
}

#[test]
fn each_market_refuses_the_types_it_does_not_take_now() {
    let venue = venue();
    let mtl = typed(OrderKind::MarketToLimit, TimeInForce::Day);
    let mok = typed(OrderKind::Market, TimeInForce::FillOrKill);

    let hose = submit(&venue, order(1, "VNM", "100", mtl, None), at(ATO + 5));
    let upcom = submit(&venue, order(2, "ACV", "100", mok, None), at(MORNING));
    let closed = submit(
        &venue,
        order(3, "VNM", "100", OrderType::LIMIT, Some("62500")),
        at(CLOSED),
    );

    assert_eq!(
        refusal(&hose).as_deref(),
        Some("HOSE takes only ATO, LO now")
    );
    assert_eq!(refusal(&upcom).as_deref(), Some("UPCOM takes only LO now"));
    assert_eq!(
        refusal(&closed).as_deref(),
        Some("HOSE takes no orders now")
    );
}

#[test]
fn an_order_entered_at_lunch_waits_for_the_afternoon() {
    let venue = venue();
    advance(&venue, at(LUNCH));

    let lunch = submit(
        &venue,
        order(1, "VNM", "100", OrderType::LIMIT, Some("62600")),
        at(LUNCH + 5),
    );
    advance(&venue, at(AFTERNOON));
    let mut afternoon = Vec::new();
    venue.step(&id("VNM"), Decimal::ZERO, at(AFTERNOON), &mut |e| {
        afternoon.push(e)
    });

    assert!(fills(&lunch, 1).is_empty());
    assert_eq!(fills(&afternoon, 1), vec![(d("100"), d("62600"))]);
}

#[test]
fn prices_move_only_in_continuous_trading() {
    let venue = venue();
    let mut seed = 7;
    let mut walked = |second| {
        venue
            .walk(&id("VNM"), at(second), &mut seed, &mut |_| {})
            .is_some()
    };

    assert!(walked(MORNING));
    assert!(!walked(ATO));
    assert!(!walked(LUNCH));
    assert!(!walked(CLOSED));
}

#[test]
fn the_close_expires_what_still_rests() {
    let venue = venue();
    advance(&venue, at(MORNING));
    submit(
        &venue,
        order(1, "VNM", "100", OrderType::LIMIT, Some("60000")),
        at(MORNING),
    );

    let closing = advance(&venue, at(CLOSED));

    assert!(ended(&closing, 1));
}

#[test]
fn a_late_beat_still_ends_the_auction_it_missed() {
    let venue = venue();
    advance(&venue, at(AFTERNOON));
    let atc = typed(OrderKind::Market, TimeInForce::AtClose);
    submit(&venue, order(1, "SHS", "100", atc, None), at(ATC + 5));

    // The next beat comes after the close: the ATC auction ended unseen.
    let late = advance(&venue, at(CLOSED));

    assert_eq!(fills(&late, 1), vec![(d("100"), d("13600"))]);
    assert!(!ended(&late, 1));
    assert!(sessions(&late).contains(&("HNX".into(), SessionPhase::Closed, None)));
}

#[test]
fn a_limit_order_after_the_close_fills_only_at_its_price_or_better() {
    // A market that takes limit orders after the close, at the closing price.
    let lo = NamedOrderType::new(OrderType::LIMIT, "LO");
    let phase = |start, phase| Phase {
        start,
        phase,
        name: None,
        order_types: vec![lo.clone()],
    };
    let day = TradingDay {
        clock: Clock::Compressed { start: START },
        markets: vec![MarketDay {
            market: "HNX".into(),
            phases: vec![phase(0, SessionPhase::AfterClose)],
        }],
    };
    let venue = MarketProfile::by_id("vn", "sim")
        .expect("vn profile")
        .venue()
        .with_day(day);
    let now = at(MORNING);

    let under = submit(
        &venue,
        order(1, "SHS", "100", OrderType::LIMIT, Some("13500")),
        now,
    );
    let over = submit(
        &venue,
        order(2, "SHS", "100", OrderType::LIMIT, Some("13700")),
        now,
    );

    assert!(fills(&under, 1).is_empty() && !ended(&under, 1));
    assert_eq!(fills(&over, 2), vec![(d("100"), d("13600"))]);
}
