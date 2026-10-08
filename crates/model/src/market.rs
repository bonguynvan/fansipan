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
    /// Halfway between bid and ask. Exact for any real price. Values whose
    /// sum would overflow, which only a broken feed sends, are halved first;
    /// at the very edge of what a `Decimal` holds, where even the halves
    /// overflow, the bid stands in. Never a panic.
    pub fn mid(&self) -> Decimal {
        self.bid
            .checked_add(self.ask)
            .map(|sum| sum / Decimal::TWO)
            .or_else(|| (self.bid / Decimal::TWO).checked_add(self.ask / Decimal::TWO))
            .unwrap_or(self.bid)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn quote(bid: Decimal, ask: Decimal) -> Quote {
        Quote {
            instrument: InstrumentId::new("sim", "AAA"),
            bid,
            ask,
            bid_size: Decimal::ONE,
            ask_size: Decimal::ONE,
            ts_event: UnixNanos(0),
        }
    }

    #[test]
    fn the_mid_is_halfway_between_bid_and_ask() {
        assert_eq!(
            quote(Decimal::new(1001, 2), Decimal::new(1003, 2)).mid(),
            Decimal::new(1002, 2)
        );
    }

    #[test]
    fn a_mid_too_large_to_add_is_not_a_panic() {
        let mid = quote(Decimal::MAX, Decimal::MAX).mid();

        assert!(mid > Decimal::ZERO);
        assert_eq!(quote(Decimal::MIN, Decimal::MAX).mid(), Decimal::ZERO);
    }
}
