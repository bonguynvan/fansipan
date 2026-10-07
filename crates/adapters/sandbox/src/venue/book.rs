//! The book a feed shows: quotes, depth, and the print that moved them.

use model::{BookLevel, Decimal, Depth, InstrumentId, Quote, Side, Trade, UnixNanos};

use super::listing::Listing;

/// Price levels published on each side of the book.
pub const DEPTH_LEVELS: usize = 3;

/// One price move on the venue: what a market-data feed publishes.
#[derive(Debug, Clone, PartialEq)]
pub struct Tick {
    pub quote: Quote,
    pub depth: Depth,
    /// The print that moved the price; `None` when it did not move.
    pub trade: Option<Trade>,
}

pub(super) struct Book {
    pub(super) mid: Decimal,
    pub(super) listing: Listing,
}

impl Book {
    pub(super) fn quote(&self, instrument: &InstrumentId, now: UnixNanos) -> Quote {
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

    pub(super) fn depth(&self, quote: &Quote) -> Depth {
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

/// The trade that moved the price: a buyer lifting the offer on the way up,
/// a seller hitting the bid on the way down.
pub(super) fn print(book: &Book, before: Decimal, quote: &Quote, now: UnixNanos) -> Option<Trade> {
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
        side: Some(side),
        ts_event: now,
    })
}
