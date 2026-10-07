//! The trading day on the venue: phase changes, the auctions that end
//! them, and the close.
//!
//! An auction matches at one price when it ends: the instrument's current
//! price on its ladder. Every order queued for the auction fills there, as
//! does every limit order that crosses it; a limit order that does not
//! cross rests on into the next phase. At the close, day orders still
//! resting are cancelled, as an exchange expires them, and the closing
//! auction's price becomes the price after-close orders fill at.

use bus::Event;
use model::{OrderEvent, SessionPhase, TimeInForce, UnixNanos};

use super::matching::{crosses, fill};
use super::{report, VenueState};

/// Moves every market with a schedule to its phase at `now`, and returns
/// what that did: the fills of an auction that ended, the cancels of day
/// orders at the close, and the new session. A market's first call only
/// announces its session.
pub(super) fn advance(state: &mut VenueState, now: UnixNanos) -> Vec<Event> {
    let Some(day) = state.day.clone() else {
        return Vec::new();
    };
    let mut events = Vec::new();
    for market in &day.markets {
        let Some(phase) = day.phase(&market.market, now) else {
            continue;
        };
        let previous = state.phases.get(&market.market).copied();
        if previous == Some(phase.phase) {
            continue;
        }
        match previous {
            Some(SessionPhase::OpeningAuction) => {
                events.extend(uncross(state, &market.market, false, now));
            }
            Some(SessionPhase::ClosingAuction) => {
                events.extend(uncross(state, &market.market, true, now));
            }
            _ => {}
        }
        if previous.is_some() && phase.phase == SessionPhase::Closed {
            events.extend(expire(state, &market.market, now));
        }
        state.phases.insert(market.market.clone(), phase.phase);
        events.push(Event::Session(phase.session(&market.market, now)));
    }
    events
}

/// Ends an auction on `market`: everything queued for it, and every limit
/// order that crosses, fills at each instrument's current price.
fn uncross(state: &mut VenueState, market: &str, closing: bool, now: UnixNanos) -> Vec<Event> {
    let prices: Vec<_> = state
        .books
        .iter()
        .filter(|(_, book)| book.listing.info.exchange == market)
        .map(|(id, book)| (id.clone(), book.mid))
        .collect();
    let mut events = Vec::new();
    for (instrument, price) in prices {
        let (matched, kept): (Vec<_>, Vec<_>) = std::mem::take(&mut state.resting)
            .into_iter()
            .partition(|resting| {
                resting.order.instrument == instrument
                    && match resting.limit {
                        Some(limit) => crosses(resting.order.side, limit, price),
                        None => matches!(
                            resting.order.order_type.time_in_force,
                            TimeInForce::AtOpen | TimeInForce::AtClose
                        ),
                    }
            });
        state.resting = kept;
        for resting in matched {
            events.extend(fill(state, &resting.order, resting.remaining, price, now));
        }
        if closing {
            state.closes.insert(instrument, price);
        }
    }
    events
}

/// The close of `market`: its resting orders expire.
fn expire(state: &mut VenueState, market: &str, now: UnixNanos) -> Vec<Event> {
    let books = &state.books;
    let (expired, kept): (Vec<_>, Vec<_>) = std::mem::take(&mut state.resting)
        .into_iter()
        .partition(|resting| {
            books
                .get(&resting.order.instrument)
                .is_some_and(|book| book.listing.info.exchange == market)
        });
    state.resting = kept;
    expired
        .into_iter()
        .map(|resting| report(resting.order.order_id, OrderEvent::Cancelled, now))
        .collect()
}
