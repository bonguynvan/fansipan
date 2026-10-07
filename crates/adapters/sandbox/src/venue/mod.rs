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

mod book;
mod listing;
mod matching;
mod session;
#[cfg(test)]
mod tests;

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use bus::Event;
use model::{
    AccountState, CancelOrder, ClientOrderId, Decimal, ExecutionReport, InstrumentId, OrderEvent,
    Position, Quote, SessionPhase, SubmitOrder, UnixNanos,
};

use crate::day::TradingDay;

use book::{print, Book};
use listing::walk_ticks;
use matching::{match_resting, phase_of, submit_locked};

pub use book::{Tick, DEPTH_LEVELS};
pub use listing::{Listing, MIN_MID, TICK, TOP_OF_BOOK_SIZE};

struct Resting {
    order: SubmitOrder,
    remaining: Decimal,
    /// The price it trades at as a maker: its own limit, or the price a
    /// market-to-limit order traded at. `None` for an order that waits for
    /// an auction to price it.
    limit: Option<Decimal>,
    /// The price per share kept back from cash while a buy rests.
    reserve: Decimal,
}

/// Shares bought that have not settled yet.
struct Unsettled {
    instrument: InstrumentId,
    quantity: Decimal,
    /// The trading day they settle on.
    settles_on: u64,
}

struct VenueState {
    books: BTreeMap<InstrumentId, Book>,
    resting: Vec<Resting>,
    cash: Decimal,
    positions: BTreeMap<InstrumentId, Position>,
    next_venue_id: u64,
    next_fill_id: u64,
    /// The markets' schedules; `None` trades every market continuously.
    day: Option<TradingDay>,
    /// Each scheduled market's phase, by its position in the market's day,
    /// as of the last [`SimVenue::advance`].
    phases: BTreeMap<String, usize>,
    /// Each instrument's price at its last closing auction.
    closes: BTreeMap<InstrumentId, Decimal>,
    /// Bought shares still settling; only kept when the venue has a day.
    unsettled: Vec<Unsettled>,
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
                day: None,
                phases: BTreeMap::new(),
                closes: BTreeMap::new(),
                unsettled: Vec::new(),
            })),
        }
    }

    /// Runs the venue's markets on `day`: each in the phase the day's clock
    /// says, taking only that phase's order types.
    pub fn with_day(self, day: TradingDay) -> Self {
        self.state().day = Some(day);
        self
    }

    /// Every scheduled market's session as the venue holds it now, as a feed
    /// announces it at connect.
    pub fn sessions(&self, now: UnixNanos) -> Vec<Event> {
        let state = self.state();
        let Some(day) = &state.day else {
            return Vec::new();
        };
        day.markets
            .iter()
            .filter_map(|market| {
                let index = state
                    .phases
                    .get(&market.market)
                    .copied()
                    .or_else(|| day.phase_index(market, now))?;
                let phase = market.phases.get(index)?;
                Some(Event::Session(phase.session(&market.market, now)))
            })
            .collect()
    }

    /// Moves each scheduled market to its phase at `now`, and sends what
    /// that did to `sink`: an ended auction's fills, the close's cancels,
    /// and the new session. Call it before each beat's prices move.
    pub fn advance(&self, now: UnixNanos, sink: &mut dyn FnMut(Event)) {
        let mut state = self.state();
        session::advance(&mut state, now).into_iter().for_each(sink);
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

    /// One beat of the simulated market for `instrument`: a step drawn from
    /// `seed`, applied as [`SimVenue::step_ticks`] does. Prices only move in
    /// continuous trading: `None` while the instrument's market is in an
    /// auction, a break, or closed, or if it is not listed.
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
            if phase_of(&state, &book.listing.info.exchange, now) != SessionPhase::Continuous {
                return None;
            }
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

/// Shares of `instrument` bought and not settled yet.
fn unsettled_in(state: &VenueState, instrument: &InstrumentId) -> Decimal {
    state
        .unsettled
        .iter()
        .filter(|lot| &lot.instrument == instrument)
        .map(|lot| lot.quantity)
        .sum()
}

/// The account as the venue reports it. With a trading day, each position
/// says how much of it has settled.
fn account_of(state: &VenueState) -> AccountState {
    let positions = state
        .positions
        .values()
        .map(|position| match state.day {
            Some(_) => position.clone().with_settled(
                (position.quantity - unsettled_in(state, &position.instrument)).max(Decimal::ZERO),
            ),
            None => position.clone(),
        })
        .collect();
    AccountState::new(state.cash, positions)
}

fn report(order_id: ClientOrderId, event: OrderEvent, now: UnixNanos) -> Event {
    Event::Execution(ExecutionReport {
        order_id,
        event,
        ts_event: now,
    })
}
