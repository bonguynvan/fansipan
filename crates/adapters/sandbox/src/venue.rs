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
//!
//! Each listing carries its market's rules as data ([`Listing`]): tick
//! sizes, lot size, today's price band, displayed size. Prices stay on the
//! tick ladder and inside the band, and orders that break a rule are
//! rejected the way an exchange would.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use bus::Event;
use model::{
    AccountState, BookLevel, CancelOrder, ClientOrderId, Decimal, Depth, ExecutionReport, FillId,
    InstrumentId, InstrumentInfo, OrderEvent, Position, PriceBand, Quote, Side, SubmitOrder,
    TickTable, Trade, UnixNanos, VenueOrderId,
};

/// Tick of an instrument listed with [`SimVenue::list`] (no market rules).
pub const TICK: Decimal = Decimal::from_parts(1, 0, 0, false, 2);
/// Lowest mid such a walk can reach: two ticks, so the bid never reaches
/// zero and nothing fills at a price of zero.
pub const MIN_MID: Decimal = Decimal::from_parts(2, 0, 0, false, 2);
/// Displayed size of such an instrument at the best bid and ask, and the
/// most one order can fill per price move.
pub const TOP_OF_BOOK_SIZE: Decimal = Decimal::ONE_HUNDRED;
/// Price levels published on each side of the book.
pub const DEPTH_LEVELS: usize = 3;
const AVG_PRICE_DP: u32 = 8;
/// One beat in this many moves a price at all; the rest leave it alone.
const MOVE_ODDS: u64 = 3;
/// How far a price strays from where it leans, as a fraction of its
/// reference price, before every move takes it back.
const PULL_RANGE: Decimal = Decimal::from_parts(5, 0, 0, false, 2);
/// How far a price may lean without daily limits, as a fraction of its
/// reference price.
const OPEN_LEAN: Decimal = Decimal::from_parts(3, 0, 0, false, 2);
/// The strongest lean, as a fraction of the way from reference to a limit.
const MAX_LEAN: Decimal = Decimal::from_parts(95, 0, 0, false, 2);
/// Resolution of the draws that pick a lean and a direction.
const DRAW_STEPS: u64 = 1_000;
/// Deeper book levels and trade prints show between 1 and this many lots.
const MAX_LOTS: u64 = 40;

/// How one instrument trades on the simulated venue.
#[derive(Debug, Clone, PartialEq)]
pub struct Listing {
    pub info: InstrumentInfo,
    /// Today's limits. Its reference is also the opening price.
    pub band: PriceBand,
    /// Size shown at the best bid and ask, and the most one order fills per
    /// price move: an order larger than this fills across several moves.
    pub display_size: Decimal,
}

impl Listing {
    /// No market rules: a flat 0.01 tick, lot of one, no price limits.
    pub fn unrestricted(instrument: InstrumentId, price: Decimal) -> Result<Self, String> {
        let ticks = TickTable::flat(TICK).map_err(|e| e.to_string())?;
        Ok(Self {
            info: InstrumentInfo::new(instrument.clone(), Decimal::ONE, ticks),
            band: PriceBand::new(instrument, price, None, None, UnixNanos::default()),
            display_size: TOP_OF_BOOK_SIZE,
        })
    }

    fn ticks(&self) -> &TickTable {
        &self.info.ticks
    }

    /// Where the mid may go so that the bid and ask stay inside the band
    /// and above zero.
    fn clamp(&self, mid: Decimal) -> Decimal {
        let ticks = self.ticks();
        let lowest = self.band.floor.map_or_else(
            || ticks.step_up(ticks.step_up(Decimal::ZERO)),
            |f| ticks.step_up(f),
        );
        let clamped = mid.max(lowest);
        match self.band.ceiling {
            Some(ceiling) => clamped.min(ticks.step_down(ceiling).unwrap_or(ceiling)),
            None => clamped,
        }
    }

    /// Where this instrument's price drifts today. Drawn from the symbol, so
    /// every run shows the same mix: most names near their reference, now
    /// and then one pressing its ceiling or floor.
    fn lean(&self) -> Decimal {
        let reference = self.band.reference;
        let draw = Decimal::from(fnv1a(self.info.id.symbol.as_bytes()) % (2 * DRAW_STEPS + 1))
            / Decimal::from(DRAW_STEPS)
            - Decimal::ONE;
        // Cubing keeps most draws small and leaves a few near the ends.
        let shaped = draw * draw * draw * MAX_LEAN;
        let reach = match (self.band.ceiling, self.band.floor) {
            (Some(ceiling), _) if shaped > Decimal::ZERO => ceiling - reference,
            (_, Some(floor)) if shaped < Decimal::ZERO => reference - floor,
            _ => reference * OPEN_LEAN,
        };
        self.clamp(self.ticks().round_nearest(reference + shaped * reach))
    }

    /// `price` moved `steps` prices along the ladder, up for positive, so a
    /// step from a tier boundary lands on the neighbouring price whichever
    /// way it goes. Not clamped to the band.
    fn moved(&self, price: Decimal, steps: i64) -> Decimal {
        let ticks = self.ticks();
        (0..steps.unsigned_abs()).fold(price, |at, _| {
            if steps > 0 {
                ticks.step_up(at)
            } else {
                ticks.step_down(at).unwrap_or(at)
            }
        })
    }

    /// A size for a deeper level or a print: a whole number of lots that is
    /// the same every time for the same price and slot.
    fn lots(&self, price: Decimal, slot: u64) -> Decimal {
        let units =
            u64::try_from(price.mantissa().unsigned_abs() % u128::from(u64::MAX)).unwrap_or(0);
        let mixed = xorshift(units ^ slot.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        self.info.lot_size.max(Decimal::ONE) * Decimal::from(1 + mixed % MAX_LOTS)
    }
}

/// One price move on the venue: what a market-data feed publishes.
#[derive(Debug, Clone, PartialEq)]
pub struct Tick {
    pub quote: Quote,
    pub depth: Depth,
    /// The print that moved the price; `None` when it did not move.
    pub trade: Option<Trade>,
}

struct Book {
    mid: Decimal,
    listing: Listing,
}

impl Book {
    fn quote(&self, instrument: &InstrumentId, now: UnixNanos) -> Quote {
        let ticks = self.listing.ticks();
        Quote {
            instrument: instrument.clone(),
            bid: ticks.step_down(self.mid).unwrap_or(self.mid),
            ask: ticks.step_up(self.mid),
            bid_size: self.listing.display_size,
            ask_size: self.listing.display_size,
            ts_event: now,
        }
    }

    fn depth(&self, quote: &Quote) -> Depth {
        let ticks = self.listing.ticks();
        let band = &self.listing.band;
        let side = |best: Decimal, next: &dyn Fn(Decimal) -> Decimal| {
            let mut levels = Vec::with_capacity(DEPTH_LEVELS);
            let mut price = best;
            for slot in 0..DEPTH_LEVELS {
                if price <= Decimal::ZERO || !band.contains(price) {
                    break;
                }
                let size = if slot == 0 {
                    self.listing.display_size
                } else {
                    self.listing.lots(price, slot as u64)
                };
                levels.push(BookLevel::new(price, size));
                price = next(price);
            }
            levels
        };
        Depth::new(
            quote.instrument.clone(),
            side(quote.bid, &|p| ticks.step_down(p).unwrap_or(Decimal::ZERO)),
            side(quote.ask, &|p| ticks.step_up(p)),
            quote.ts_event,
        )
    }
}

struct Resting {
    order: SubmitOrder,
    remaining: Decimal,
}

struct VenueState {
    books: BTreeMap<InstrumentId, Book>,
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
                books: BTreeMap::new(),
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

    /// Lists an instrument with no market rules at `mid`.
    pub fn list(&self, instrument: InstrumentId, mid: Decimal) {
        if let Ok(listing) = Listing::unrestricted(instrument, mid) {
            self.list_with(listing);
        }
    }

    /// Lists an instrument under its market's rules, opening at the band's
    /// reference price.
    pub fn list_with(&self, listing: Listing) {
        let mid = listing.clamp(listing.ticks().round_nearest(listing.band.reference));
        self.state()
            .books
            .insert(listing.info.id.clone(), Book { mid, listing });
    }

    pub fn is_listed(&self, instrument: &InstrumentId) -> bool {
        self.state().books.contains_key(instrument)
    }

    pub fn instruments(&self) -> Vec<InstrumentId> {
        self.state().books.keys().cloned().collect()
    }

    pub fn quote(&self, instrument: &InstrumentId, now: UnixNanos) -> Option<Quote> {
        self.state()
            .books
            .get(instrument)
            .map(|book| book.quote(instrument, now))
    }

    /// Every listing's rules and today's band, as a feed announces them.
    pub fn reference_data(&self, now: UnixNanos) -> Vec<Event> {
        self.state()
            .books
            .values()
            .flat_map(|book| {
                [
                    Event::Instrument(book.listing.info.clone()),
                    Event::PriceBand({
                        let mut band = book.listing.band.clone();
                        band.ts_event = now;
                        band
                    }),
                ]
            })
            .collect()
    }

    /// A made-up past for one instrument: `count` quotes `spacing` apart,
    /// the last one a `spacing` before `end`, walking backwards from the
    /// current mid so the history joins the live feed without a jump. It
    /// stays on the tick ladder and inside the band. Read-only: the venue's
    /// state does not change. Empty if the instrument is not listed.
    pub fn synthetic_history(
        &self,
        instrument: &InstrumentId,
        end: UnixNanos,
        count: usize,
        spacing: Duration,
        seed: &mut u64,
    ) -> Vec<Quote> {
        let state = self.state();
        let Some(book) = state.books.get(instrument) else {
            return Vec::new();
        };
        let spacing = u64::try_from(spacing.as_nanos()).unwrap_or(u64::MAX);
        let mut mids = Vec::with_capacity(count);
        let mut price = book.mid;
        let lean = book.listing.lean();
        let reference = book.listing.band.reference;
        for _ in 0..count {
            mids.push(price);
            // The walk leans back toward the same price either way in time,
            // so walking it from now into the past gives a plausible history.
            let step = walk_ticks(seed, price, lean, reference);
            price = book.listing.clamp(book.listing.moved(price, step));
        }
        mids.into_iter()
            .rev()
            .enumerate()
            .map(|(index, mid)| {
                let steps_back = u64::try_from(count - index).unwrap_or(u64::MAX);
                let ts = end.0.saturating_sub(steps_back.saturating_mul(spacing));
                let past = Book {
                    mid,
                    listing: book.listing.clone(),
                };
                past.quote(instrument, UnixNanos(ts))
            })
            .collect()
    }

    pub fn account(&self) -> AccountState {
        account_of(&self.state())
    }

    /// Moves the mid by `delta` (rounded to the tick ladder, kept inside the
    /// band), sends the fills for whatever resting orders the new price
    /// reaches to `sink`, and returns what a feed would publish. `None` if
    /// the instrument is not listed.
    pub fn step(
        &self,
        instrument: &InstrumentId,
        delta: Decimal,
        now: UnixNanos,
        sink: &mut dyn FnMut(Event),
    ) -> Option<Tick> {
        let mut state = self.state();
        let tick = {
            let book = state.books.get_mut(instrument)?;
            let before = book.mid;
            let moved = before.checked_add(delta).unwrap_or(before);
            book.mid = book
                .listing
                .clamp(book.listing.ticks().round_nearest(moved));
            let quote = book.quote(instrument, now);
            let depth = book.depth(&quote);
            let trade = print(book, before, &quote, now);
            Tick {
                quote,
                depth,
                trade,
            }
        };
        match_resting(&mut state, &tick.quote, now)
            .into_iter()
            .for_each(sink);
        Some(tick)
    }

    /// One beat of the simulated market for `instrument`: a step drawn by
    /// [`walk_ticks`] from `seed`, applied as [`SimVenue::step_ticks`] does.
    pub fn walk(
        &self,
        instrument: &InstrumentId,
        now: UnixNanos,
        seed: &mut u64,
        sink: &mut dyn FnMut(Event),
    ) -> Option<Tick> {
        let ticks = {
            let state = self.state();
            let book = state.books.get(instrument)?;
            walk_ticks(
                seed,
                book.mid,
                book.listing.lean(),
                book.listing.band.reference,
            )
        };
        self.step_ticks(instrument, ticks, now, sink)
    }

    /// Moves the mid `ticks` prices along its ladder.
    pub fn step_ticks(
        &self,
        instrument: &InstrumentId,
        ticks: i64,
        now: UnixNanos,
        sink: &mut dyn FnMut(Event),
    ) -> Option<Tick> {
        let delta = {
            let state = self.state();
            let book = state.books.get(instrument)?;
            book.listing
                .moved(book.mid, ticks)
                .checked_sub(book.mid)
                .unwrap_or(Decimal::ZERO)
        };
        self.step(instrument, delta, now, sink)
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

/// The trade that moved the price: a buyer lifting the offer on the way up,
/// a seller hitting the bid on the way down.
fn print(book: &Book, before: Decimal, quote: &Quote, now: UnixNanos) -> Option<Trade> {
    let (price, side) = if book.mid > before {
        (quote.ask, Side::Buy)
    } else if book.mid < before {
        (quote.bid, Side::Sell)
    } else {
        return None;
    };
    Some(Trade {
        instrument: quote.instrument.clone(),
        price,
        quantity: book.listing.lots(price, 7),
        side,
        ts_event: now,
    })
}

/// Why the venue would refuse this order under the listing's rules, if it would.
fn rule_breach(listing: &Listing, order: &SubmitOrder) -> Option<String> {
    if !listing.info.is_lot_multiple(order.quantity) {
        return Some(format!(
            "quantity must be a multiple of the lot size, {}",
            listing.info.lot_size
        ));
    }
    let price = order.limit_price?;
    if !listing.ticks().is_on_tick(price) {
        return Some(format!(
            "price must be a multiple of the tick size, {}",
            listing.ticks().tick_at(price)
        ));
    }
    if !listing.band.contains(price) {
        return Some("price is outside today's ceiling and floor".into());
    }
    None
}

fn submit_locked(state: &mut VenueState, order: SubmitOrder, now: UnixNanos) -> Vec<Event> {
    let reject =
        |reason: String| vec![report(order.order_id, OrderEvent::Rejected { reason }, now)];

    let Some(book) = state.books.get(&order.instrument) else {
        return reject(format!("{} is not listed on this venue", order.instrument));
    };
    if order.quantity <= Decimal::ZERO {
        return reject("quantity must be positive".into());
    }
    if let Some(reason) = rule_breach(&book.listing, &order) {
        return reject(reason);
    }
    let quote = book.quote(&order.instrument, now);
    let display_size = book.listing.display_size;
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
            Some(_) => remaining.min(display_size),
        };
        events.extend(fill(state, &order, quantity, touch, now));
        remaining -= quantity;
    }
    if remaining > Decimal::ZERO {
        state.resting.push(Resting { order, remaining });
    }
    events
}

/// One beat of the walk, in ticks: usually none, otherwise one either way,
/// back toward `lean` more often the further `mid` has strayed from it (a
/// full `PULL_RANGE` of `reference` away, always back). Drawn from a
/// xorshift sequence, so a given seed always walks the same path.
pub(crate) fn walk_ticks(seed: &mut u64, mid: Decimal, lean: Decimal, reference: Decimal) -> i64 {
    *seed = xorshift(*seed);
    if !seed.is_multiple_of(MOVE_ODDS) {
        return 0;
    }
    *seed = xorshift(*seed);
    let stray = mid
        .checked_sub(lean)
        .zip(reference.checked_mul(PULL_RANGE))
        .and_then(|(distance, range)| distance.checked_div(range))
        .unwrap_or(Decimal::ZERO);
    let pull = stray.clamp(-Decimal::ONE, Decimal::ONE);
    // An up-move's chance in DRAW_STEPS: half at the lean, none a full range above it.
    let up = (Decimal::ONE - pull) * Decimal::from(DRAW_STEPS / 2);
    if Decimal::from(*seed % DRAW_STEPS) < up {
        1
    } else {
        -1
    }
}

/// FNV-1a: a stable hash, the same on every platform and every run.
fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

fn xorshift(mut x: u64) -> u64 {
    x ^= x << 13;
    x ^= x >> 7;
    x ^ (x << 17)
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

/// Fills resting orders the new quote reaches, at most the displayed size
/// each. A resting order is the maker, so it trades at its own limit price,
/// not at the touch.
fn match_resting(state: &mut VenueState, quote: &Quote, now: UnixNanos) -> Vec<Event> {
    let display_size = state
        .books
        .get(&quote.instrument)
        .map_or(TOP_OF_BOOK_SIZE, |book| book.listing.display_size);
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
        let quantity = resting.remaining.min(display_size);
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
        let band =
            PriceBand::from_fraction(id.clone(), d(reference), d("0.07"), &ticks, NOW).unwrap();
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
}
