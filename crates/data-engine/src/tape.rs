//! The tape: an instrument's most recent prints, for time and sales. Each
//! print is numbered in the order the tape took it, so a consumer holding
//! some prints from a read and some from the live feed can tell which it
//! already has.

use std::collections::VecDeque;

use model::Trade;

/// A print as the tape keeps it.
#[derive(Debug, Clone, PartialEq)]
pub struct Print {
    /// One more than the instrument's print before it, from 1. A tape
    /// started again after an unsubscribe carries on the count, so a
    /// number never names two prints and consumers never have to tell a
    /// new tape from an old one.
    pub seq: u64,
    pub trade: Trade,
}

/// The most recent `capacity` prints of one instrument, oldest first.
#[derive(Debug, Clone)]
pub struct Tape {
    capacity: usize,
    taken: u64,
    prints: VecDeque<Print>,
}

impl Tape {
    /// A capacity of zero is treated as one, so a tape always holds the
    /// last print.
    pub fn new(capacity: usize) -> Self {
        Self::after(capacity, 0)
    }

    /// A tape that carries on an earlier one's count: its first print is
    /// numbered `taken + 1`.
    pub fn after(capacity: usize, taken: u64) -> Self {
        Self {
            capacity: capacity.max(1),
            taken,
            prints: VecDeque::new(),
        }
    }

    /// Prints numbered so far, by this tape and any it carries on from.
    pub fn taken(&self) -> u64 {
        self.taken
    }

    /// Takes the next print, dropping the oldest once full.
    pub fn record(&mut self, trade: Trade) {
        if self.prints.len() == self.capacity {
            self.prints.pop_front();
        }
        self.taken = self.taken.saturating_add(1);
        self.prints.push_back(Print {
            seq: self.taken,
            trade,
        });
    }

    pub fn last(&self) -> Option<&Print> {
        self.prints.back()
    }

    /// The newest `limit` prints, oldest first.
    pub fn recent(&self, limit: usize) -> impl Iterator<Item = &Print> {
        self.prints
            .iter()
            .skip(self.prints.len().saturating_sub(limit))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use model::{Decimal, InstrumentId, Side, UnixNanos};

    fn trade(price: i64) -> Trade {
        Trade {
            instrument: InstrumentId::new("sim", "AAA"),
            price: Decimal::new(price, 0),
            quantity: Decimal::ONE_HUNDRED,
            side: Some(Side::Buy),
            ts_event: UnixNanos(0),
        }
    }

    fn seqs<'a>(prints: impl Iterator<Item = &'a Print>) -> Vec<u64> {
        prints.map(|p| p.seq).collect()
    }

    #[test]
    fn prints_are_numbered_in_the_order_taken() {
        let mut tape = Tape::new(10);
        tape.record(trade(10));
        tape.record(trade(11));

        assert_eq!(seqs(tape.recent(10)), vec![1, 2]);
        assert_eq!(
            tape.last().map(|p| p.trade.price),
            Some(Decimal::new(11, 0))
        );
    }

    #[test]
    fn a_full_tape_drops_its_oldest_print_but_keeps_counting() {
        let mut tape = Tape::new(2);
        for price in 10..14 {
            tape.record(trade(price));
        }

        assert_eq!(seqs(tape.recent(10)), vec![3, 4]);
    }

    #[test]
    fn recent_gives_the_newest_prints_oldest_first() {
        let mut tape = Tape::new(10);
        for price in 10..15 {
            tape.record(trade(price));
        }

        assert_eq!(seqs(tape.recent(2)), vec![4, 5]);
        assert_eq!(seqs(tape.recent(0)), Vec::<u64>::new());
    }

    #[test]
    fn a_tape_carrying_on_numbers_after_the_earlier_one() {
        let mut tape = Tape::after(10, 41);
        tape.record(trade(10));

        assert_eq!(seqs(tape.recent(10)), vec![42]);
        assert_eq!(tape.taken(), 42);
    }

    #[test]
    fn a_zero_capacity_still_keeps_the_last_print() {
        let mut tape = Tape::new(0);
        tape.record(trade(10));
        tape.record(trade(11));

        assert_eq!(seqs(tape.recent(10)), vec![2]);
    }
}
