//! The simulated venue: prices, a resting-order book, and one cash account.
//! Every method takes the time as an argument and hands the events it
//! produced to a sink, so tests drive it deterministically and the client
//! wrappers decide where events go.
//!
//! Events reach the sink while the venue's lock is still held. The market
//! thread and the order thread both publish, and without that a fill from a
//! price move could overtake the `Accepted` it follows, or an older account
//! snapshot could land after a newer one. The bus never blocks, so holding
//! the lock across a publish costs nothing.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use bus::Event;
use model::{
    AccountState, CancelOrder, ClientOrderId, Decimal, ExecutionReport, FillId, InstrumentId,
    OrderEvent, Position, Quote, Side, SubmitOrder, UnixNanos, VenueOrderId,
};

/// Bid/ask distance around the simulated mid, in price units.
pub const SPREAD: Decimal = Decimal::from_parts(2, 0, 0, false, 2);
/// Smallest price increment.
pub const TICK: Decimal = Decimal::from_parts(1, 0, 0, false, 2);
/// Lowest mid a random walk can reach: one tick plus half the spread, so the
/// bid never reaches zero and nothing ever fills at a price of zero.
pub const MIN_MID: Decimal = Decimal::from_parts(2, 0, 0, false, 2);
/// Displayed size on each side, and the most one order can fill per price
/// move. A resting order larger than this fills across several ticks.
pub const TOP_OF_BOOK_SIZE: Decimal = Decimal::ONE_HUNDRED;
const AVG_PRICE_DP: u32 = 8;
/// A random step moves a price by up to this many ticks either way.
const MAX_STEP_TICKS: u64 = 2;

struct Resting {
    order: SubmitOrder,
    remaining: Decimal,
}

struct VenueState {
    mids: BTreeMap<InstrumentId, Decimal>,
    resting: Vec<Resting>,
    cash: Decimal,
    positions: BTreeMap<InstrumentId, Position>,
    next_venue_id: u64,
    next_fill_id: u64,
}

/// Cheap to clone; every clone is the same venue.
#[derive(Clone)]
pub struct SimVenue {
    state: Arc<Mutex<VenueState>>,
}

impl SimVenue {
    pub fn new(starting_cash: Decimal) -> Self {
        Self {
            state: Arc::new(Mutex::new(VenueState {
                mids: BTreeMap::new(),
                resting: Vec::new(),
                cash: starting_cash,
                positions: BTreeMap::new(),
                next_venue_id: 1,
                next_fill_id: 1,
            })),
        }
    }

    /// A panic inside one venue call can at worst leave one simulated fill
    /// half-booked. For a simulator that is better than refusing every
    /// later call, so a poisoned lock is recovered rather than propagated.
    fn state(&self) -> MutexGuard<'_, VenueState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn list(&self, instrument: InstrumentId, mid: Decimal) {
        self.state().mids.insert(instrument, mid);
    }

    pub fn is_listed(&self, instrument: &InstrumentId) -> bool {
        self.state().mids.contains_key(instrument)
    }

    pub fn instruments(&self) -> Vec<InstrumentId> {
        self.state().mids.keys().cloned().collect()
    }

    pub fn quote(&self, instrument: &InstrumentId, now: UnixNanos) -> Option<Quote> {
        self.state()
            .mids
            .get(instrument)
            .map(|mid| quote_at(instrument, *mid, now))
    }

    /// A made-up past for one instrument: `count` quotes `spacing` apart,
    /// the last one a `spacing` before `end`, walking backwards from the
    /// current mid so the history joins the live feed without a jump.
    /// Read-only: the venue's state does not change. Empty if the
    /// instrument is not listed.
    pub fn synthetic_history(
        &self,
        instrument: &InstrumentId,
        end: UnixNanos,
        count: usize,
        spacing: Duration,
        seed: &mut u64,
    ) -> Vec<Quote> {
        let Some(mid) = self.state().mids.get(instrument).copied() else {
            return Vec::new();
        };
        let spacing = u64::try_from(spacing.as_nanos()).unwrap_or(u64::MAX);
        let mut mids = Vec::with_capacity(count);
        let mut price = mid;
        for _ in 0..count {
            mids.push(price);
            price = price
                .checked_sub(random_step(seed))
                .unwrap_or(price)
                .max(MIN_MID);
        }
        mids.into_iter()
            .rev()
            .enumerate()
            .map(|(index, mid)| {
                let steps_back = u64::try_from(count - index).unwrap_or(u64::MAX);
                let ts = end.0.saturating_sub(steps_back.saturating_mul(spacing));
                quote_at(instrument, mid, UnixNanos(ts))
            })
            .collect()
    }

    pub fn account(&self) -> AccountState {
        account_of(&self.state())
    }

    /// Moves the mid by `delta` (never below [`MIN_MID`]), sends the fills
    /// for whatever resting orders the new price reaches to `sink`, and
    /// returns the new quote. `None` if the instrument is not listed.
    pub fn step(
        &self,
        instrument: &InstrumentId,
        delta: Decimal,
        now: UnixNanos,
        sink: &mut dyn FnMut(Event),
    ) -> Option<Quote> {
        let mut state = self.state();
        let mid = state.mids.get_mut(instrument)?;
        *mid = mid.checked_add(delta).unwrap_or(*mid).max(MIN_MID);
        let quote = quote_at(instrument, *mid, now);
        match_resting(&mut state, &quote, now)
            .into_iter()
            .for_each(sink);
        Some(quote)
    }

    pub fn submit(&self, order: SubmitOrder, now: UnixNanos, sink: &mut dyn FnMut(Event)) {
        let mut state = self.state();
        submit_locked(&mut state, order, now)
            .into_iter()
            .for_each(sink);
    }

    /// Cancelling something that is not resting is answered with
    /// `CancelRejected`, never an error: the order may simply have filled.
    pub fn cancel(&self, cancel: &CancelOrder, now: UnixNanos, sink: &mut dyn FnMut(Event)) {
        let mut state = self.state();
        let before = state.resting.len();
        state
            .resting
            .retain(|r| r.order.order_id != cancel.order_id);
        let event = if state.resting.len() < before {
            OrderEvent::Cancelled
        } else {
            OrderEvent::CancelRejected {
                reason: "order is not open on the venue".into(),
            }
        };
        sink(report(cancel.order_id, event, now));
    }
}

fn submit_locked(state: &mut VenueState, order: SubmitOrder, now: UnixNanos) -> Vec<Event> {
    let reject =
        |reason: String| vec![report(order.order_id, OrderEvent::Rejected { reason }, now)];

    let Some(mid) = state.mids.get(&order.instrument).copied() else {
        return reject(format!("{} is not listed on this venue", order.instrument));
    };
    if order.quantity <= Decimal::ZERO {
        return reject("quantity must be positive".into());
    }
    let quote = quote_at(&order.instrument, mid, now);
    let touch = touch_price(&quote, order.side);

    // Every later fill of this order trades at or inside `worst_price`, so
    // bounding its value here keeps the plain arithmetic in `fill` safe.
    let worst_price = order.limit_price.unwrap_or(touch);
    let notional = order
        .quantity
        .checked_mul(worst_price)
        .filter(|n| state.cash.checked_add(*n).is_some() && state.cash.checked_sub(*n).is_some());
    let Some(notional) = notional else {
        return reject("order value is out of range".into());
    };
    if order.side == Side::Buy {
        let available = available_cash(state);
        if available.is_none_or(|available| notional > available) {
            return reject(format!(
                "insufficient cash: needs {notional}, {} available",
                available.unwrap_or_default()
            ));
        }
    }

    let venue_order_id = VenueOrderId(format!("SIM-{}", state.next_venue_id));
    state.next_venue_id += 1;
    let mut events = vec![report(
        order.order_id,
        OrderEvent::Accepted { venue_order_id },
        now,
    )];

    let marketable = order
        .limit_price
        .is_none_or(|limit| crosses(order.side, limit, touch));
    let mut remaining = order.quantity;
    if marketable {
        // A market order sweeps; a marketable limit takes what is shown.
        let quantity = match order.limit_price {
            None => remaining,
            Some(_) => remaining.min(TOP_OF_BOOK_SIZE),
        };
        events.extend(fill(state, &order, quantity, touch, now));
        remaining -= quantity;
    }
    if remaining > Decimal::ZERO {
        state.resting.push(Resting { order, remaining });
    }
    events
}

/// A price move of up to `MAX_STEP_TICKS` ticks either way, drawn from a
/// xorshift sequence so a given seed always walks the same path.
pub(crate) fn random_step(seed: &mut u64) -> Decimal {
    *seed = xorshift(*seed);
    let ticks = Decimal::from(*seed % (2 * MAX_STEP_TICKS + 1)) - Decimal::from(MAX_STEP_TICKS);
    ticks * TICK
}

fn xorshift(mut x: u64) -> u64 {
    x ^= x << 13;
    x ^= x >> 7;
    x ^ (x << 17)
}

fn quote_at(instrument: &InstrumentId, mid: Decimal, now: UnixNanos) -> Quote {
    let half = SPREAD / Decimal::TWO;
    Quote {
        instrument: instrument.clone(),
        bid: mid - half,
        ask: mid + half,
        bid_size: TOP_OF_BOOK_SIZE,
        ask_size: TOP_OF_BOOK_SIZE,
        ts_event: now,
    }
}

/// The price a taker on `side` trades at.
fn touch_price(quote: &Quote, side: Side) -> Decimal {
    match side {
        Side::Buy => quote.ask,
        Side::Sell => quote.bid,
    }
}

fn crosses(side: Side, limit: Decimal, touch: Decimal) -> bool {
    match side {
        Side::Buy => limit >= touch,
        Side::Sell => limit <= touch,
    }
}

/// Cash not already promised to resting buy orders at their limit price.
/// `None` only if the reservations themselves overflow.
fn available_cash(state: &VenueState) -> Option<Decimal> {
    state
        .resting
        .iter()
        .filter(|r| r.order.side == Side::Buy)
        .try_fold(state.cash, |available, r| {
            let reserved = r
                .remaining
                .checked_mul(r.order.limit_price.unwrap_or_default())?;
            available.checked_sub(reserved)
        })
}

/// Fills resting orders the new quote reaches. A resting order is the maker,
/// so it trades at its own limit price, not at the touch.
fn match_resting(state: &mut VenueState, quote: &Quote, now: UnixNanos) -> Vec<Event> {
    let mut fills = Vec::new();
    for index in 0..state.resting.len() {
        let resting = &state.resting[index];
        if resting.order.instrument != quote.instrument {
            continue;
        }
        let Some(limit) = resting.order.limit_price else {
            continue;
        };
        if !crosses(
            resting.order.side,
            limit,
            touch_price(quote, resting.order.side),
        ) {
            continue;
        }
        let quantity = resting.remaining.min(TOP_OF_BOOK_SIZE);
        fills.push((index, quantity, limit));
    }

    let mut events = Vec::new();
    for (index, quantity, price) in &fills {
        let order = state.resting[*index].order.clone();
        state.resting[*index].remaining -= *quantity;
        events.extend(fill(state, &order, *quantity, *price, now));
    }
    state.resting.retain(|r| r.remaining > Decimal::ZERO);
    events
}

/// Books one fill and returns the execution report plus the account update.
fn fill(
    state: &mut VenueState,
    order: &SubmitOrder,
    quantity: Decimal,
    price: Decimal,
    now: UnixNanos,
) -> Vec<Event> {
    let signed = match order.side {
        Side::Buy => quantity,
        Side::Sell => -quantity,
    };
    state.cash -= signed * price;
    let next = net_position(
        state.positions.get(&order.instrument),
        &order.instrument,
        signed,
        price,
    );
    match next {
        Some(position) => state.positions.insert(order.instrument.clone(), position),
        None => state.positions.remove(&order.instrument),
    };

    let fill_id = FillId(format!("SIM-F{}", state.next_fill_id));
    state.next_fill_id += 1;
    vec![
        report(
            order.order_id,
            OrderEvent::Filled {
                fill_id,
                quantity,
                price,
            },
            now,
        ),
        Event::AccountUpdate(account_of(state)),
    ]
}

/// The position after a signed fill, or `None` once it is flat. Adding to a
/// position blends the average price; reducing keeps it; flipping through
/// zero starts the new side at the fill price.
fn net_position(
    current: Option<&Position>,
    instrument: &InstrumentId,
    signed: Decimal,
    price: Decimal,
) -> Option<Position> {
    let (quantity, avg) = current.map_or((Decimal::ZERO, Decimal::ZERO), |p| {
        (p.quantity, p.avg_price)
    });
    let next = quantity + signed;
    if next.is_zero() {
        return None;
    }
    let adding = quantity.is_zero() || quantity.is_sign_positive() == signed.is_sign_positive();
    let flipped = !adding && next.is_sign_positive() != quantity.is_sign_positive();
    let avg_price = if adding {
        ((quantity * avg + signed * price) / next).round_dp(AVG_PRICE_DP)
    } else if flipped {
        price
    } else {
        avg
    };
    Some(Position {
        instrument: instrument.clone(),
        quantity: next,
        avg_price,
    })
}

fn account_of(state: &VenueState) -> AccountState {
    AccountState {
        cash: state.cash,
        positions: state.positions.values().cloned().collect(),
    }
}

fn report(order_id: ClientOrderId, event: OrderEvent, now: UnixNanos) -> Event {
    Event::Execution(ExecutionReport {
        order_id,
        event,
        ts_event: now,
    })
}

#[cfg(test)]
mod tests {
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
        let quote = venue
            .step(&aaa(), d(delta), NOW, &mut |e| out.push(e))
            .expect("listed");
        (quote, out)
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
}
