//! OHLC bars built from quotes. One [`BarSeries`] per subscribed instrument,
//! fed the quote mid. Volume is the number of quotes in the bar ("tick
//! volume"), because a quote feed carries no traded volume.

use std::collections::VecDeque;
use std::time::Duration;

use model::{Decimal, UnixNanos};

#[derive(Debug, Clone, PartialEq)]
pub struct Bar {
    /// Start of the bar's interval.
    pub ts_open: UnixNanos,
    pub open: Decimal,
    pub high: Decimal,
    pub low: Decimal,
    pub close: Decimal,
    /// Quotes folded into this bar.
    pub ticks: u64,
}

impl Bar {
    fn opened_at(ts_open: UnixNanos, price: Decimal) -> Self {
        Self {
            ts_open,
            open: price,
            high: price,
            low: price,
            close: price,
            ticks: 1,
        }
    }

    fn with(&self, price: Decimal) -> Self {
        Self {
            high: self.high.max(price),
            low: self.low.min(price),
            close: price,
            ticks: self.ticks + 1,
            ..self.clone()
        }
    }
}

/// The most recent `capacity` bars of one instrument, oldest first.
#[derive(Debug, Clone)]
pub struct BarSeries {
    interval_nanos: u64,
    capacity: usize,
    bars: VecDeque<Bar>,
}

impl BarSeries {
    /// An interval under one nanosecond is treated as one nanosecond, and a
    /// capacity of zero as one, so a series always holds the bar in progress.
    pub fn new(interval: Duration, capacity: usize) -> Self {
        let interval_nanos = u64::try_from(interval.as_nanos())
            .unwrap_or(u64::MAX)
            .max(1);
        Self {
            interval_nanos,
            capacity: capacity.max(1),
            bars: VecDeque::new(),
        }
    }

    /// Folds one price in. Returns the bar it changed, or `None` for a price
    /// older than the bar in progress: late data never rewrites a closed bar.
    /// Intervals with no prices produce no bar; the series stays sparse.
    pub fn on_price(&mut self, price: Decimal, ts: UnixNanos) -> Option<&Bar> {
        let bucket = UnixNanos(ts.0 - ts.0 % self.interval_nanos);
        match self.bars.back_mut() {
            Some(last) if bucket < last.ts_open => return None,
            Some(last) if bucket == last.ts_open => *last = last.with(price),
            _ => {
                if self.bars.len() == self.capacity {
                    self.bars.pop_front();
                }
                self.bars.push_back(Bar::opened_at(bucket, price));
            }
        }
        self.bars.back()
    }

    pub fn last(&self) -> Option<&Bar> {
        self.bars.back()
    }

    /// The newest `limit` bars, oldest first.
    pub fn recent(&self, limit: usize) -> impl Iterator<Item = &Bar> {
        self.bars.iter().skip(self.bars.len().saturating_sub(limit))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECOND: u64 = 1_000_000_000;

    fn series() -> BarSeries {
        BarSeries::new(Duration::from_secs(5), 3)
    }

    fn at(seconds: u64) -> UnixNanos {
        UnixNanos(seconds * SECOND)
    }

    fn p(raw: &str) -> Decimal {
        raw.parse().expect("test decimal")
    }

    #[test]
    fn prices_in_one_interval_build_one_bar() {
        let mut bars = series();

        bars.on_price(p("10"), at(100));
        bars.on_price(p("12"), at(101));
        bars.on_price(p("9"), at(103));
        let bar = bars.on_price(p("11"), at(104)).cloned().unwrap();

        assert_eq!(bar.ts_open, at(100));
        assert_eq!(
            (bar.open, bar.high, bar.low, bar.close),
            (p("10"), p("12"), p("9"), p("11"))
        );
        assert_eq!(bar.ticks, 4);
    }

    #[test]
    fn bars_open_on_interval_boundaries_not_on_the_first_price() {
        let mut bars = series();

        let bar = bars.on_price(p("10"), at(103)).cloned().unwrap();

        assert_eq!(bar.ts_open, at(100));
    }

    #[test]
    fn crossing_a_boundary_opens_a_new_bar_and_leaves_the_old_one_closed() {
        let mut bars = series();
        bars.on_price(p("10"), at(100));

        let next = bars.on_price(p("11"), at(105)).cloned().unwrap();

        assert_eq!(next.ts_open, at(105));
        assert_eq!(next.open, p("11"));
        assert_eq!(bars.recent(10).count(), 2);
    }

    #[test]
    fn a_late_price_does_not_rewrite_a_closed_bar() {
        let mut bars = series();
        bars.on_price(p("10"), at(100));
        bars.on_price(p("11"), at(105));

        let late = bars.on_price(p("99"), at(104));

        assert!(late.is_none());
        assert_eq!(bars.recent(10).next().map(|b| b.high), Some(p("10")));
    }

    #[test]
    fn quiet_intervals_leave_gaps_rather_than_flat_bars() {
        let mut bars = series();
        bars.on_price(p("10"), at(100));

        bars.on_price(p("11"), at(120));

        let opens: Vec<UnixNanos> = bars.recent(10).map(|b| b.ts_open).collect();
        assert_eq!(opens, vec![at(100), at(120)]);
    }

    #[test]
    fn the_oldest_bar_is_dropped_at_capacity() {
        let mut bars = series();

        for seconds in [100, 105, 110, 115] {
            bars.on_price(p("10"), at(seconds));
        }

        let opens: Vec<UnixNanos> = bars.recent(10).map(|b| b.ts_open).collect();
        assert_eq!(opens, vec![at(105), at(110), at(115)]);
    }

    #[test]
    fn recent_returns_the_newest_bars_oldest_first() {
        let mut bars = series();
        for seconds in [100, 105, 110] {
            bars.on_price(p("10"), at(seconds));
        }

        let opens: Vec<UnixNanos> = bars.recent(2).map(|b| b.ts_open).collect();

        assert_eq!(opens, vec![at(105), at(110)]);
    }
}
