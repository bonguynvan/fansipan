use rust_decimal::Decimal;

use crate::{InstrumentId, Side, UnixNanos};

/// Top of book for one instrument.
#[derive(Debug, Clone, PartialEq)]
pub struct Quote {
    pub instrument: InstrumentId,
    pub bid: Decimal,
    pub ask: Decimal,
    pub bid_size: Decimal,
    pub ask_size: Decimal,
    pub ts_event: UnixNanos,
}

impl Quote {
    pub fn mid(&self) -> Decimal {
        (self.bid + self.ask) / Decimal::TWO
    }
}

/// A print on the venue's tape: someone else's trade, not one of our fills.
#[derive(Debug, Clone, PartialEq)]
pub struct Trade {
    pub instrument: InstrumentId,
    pub price: Decimal,
    pub quantity: Decimal,
    /// The aggressor's side, when the venue says. Many venues' tapes do not.
    pub side: Option<Side>,
    pub ts_event: UnixNanos,
}
