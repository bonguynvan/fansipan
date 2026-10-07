//! Conflates engine output into one message per flush for the front end.
//!
//! A feed can produce thousands of events a second; a screen shows one state
//! per frame. Between flushes only the latest quote per symbol and the latest
//! state per order survive, and each changed bar is kept once, so the webview
//! receives a bounded number of messages whatever the feed rate.

use std::collections::BTreeMap;

use serde::Serialize;

use crate::dto::{AccountDto, BarDto, OrderDto, QuoteDto};

#[derive(Default)]
pub struct Batch {
    quotes: BTreeMap<String, QuoteDto>,
    orders: BTreeMap<u64, OrderDto>,
    account: Option<AccountDto>,
    bars: BTreeMap<(String, u64), BarDto>,
}

/// What one flush delivers. Bars are ordered by symbol, then time, so a
/// consumer can treat every bar but a symbol's last as closed.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct BatchDto {
    /// Increases by one per batch, so the front end can order batches
    /// against its startup snapshot and drop any it has already seen.
    pub seq: u64,
    pub quotes: Vec<QuoteDto>,
    pub orders: Vec<OrderDto>,
    pub account: Option<AccountDto>,
    pub bars: Vec<BarDto>,
}

impl Batch {
    pub fn quote(&mut self, quote: QuoteDto) {
        self.quotes.insert(quote.symbol.clone(), quote);
    }

    pub fn order(&mut self, order: OrderDto) {
        self.orders.insert(order.id, order);
    }

    pub fn account(&mut self, account: AccountDto) {
        self.account = Some(account);
    }

    /// A bar crossing into a new interval leaves the old one in the batch
    /// too, so its final state is delivered even if both change between
    /// two flushes.
    pub fn bar(&mut self, bar: BarDto) {
        self.bars.insert((bar.symbol.clone(), bar.time_ms), bar);
    }

    pub fn is_empty(&self) -> bool {
        self.quotes.is_empty()
            && self.orders.is_empty()
            && self.account.is_none()
            && self.bars.is_empty()
    }

    pub fn into_dto(self, seq: u64) -> BatchDto {
        BatchDto {
            seq,
            quotes: self.quotes.into_values().collect(),
            orders: self.orders.into_values().collect(),
            account: self.account,
            bars: self.bars.into_values().collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quote(symbol: &str, bid: &str) -> QuoteDto {
        QuoteDto {
            symbol: symbol.into(),
            bid: bid.into(),
            ask: bid.into(),
            bid_size: "1".into(),
            ask_size: "1".into(),
            ts_ms: 0,
        }
    }

    fn bar(symbol: &str, time_ms: u64, close: &str) -> BarDto {
        BarDto {
            symbol: symbol.into(),
            time_ms,
            open: "1".into(),
            high: "1".into(),
            low: "1".into(),
            close: close.into(),
            ticks: 1,
        }
    }

    #[test]
    fn only_the_latest_quote_per_symbol_survives() {
        let mut batch = Batch::default();

        batch.quote(quote("AAA", "1"));
        batch.quote(quote("BBB", "5"));
        batch.quote(quote("AAA", "2"));

        let dto = batch.into_dto(1);
        assert_eq!(dto.quotes.len(), 2);
        assert_eq!(dto.quotes[0].bid, "2");
    }

    #[test]
    fn every_bar_that_changed_is_kept_once_in_time_order() {
        let mut batch = Batch::default();

        batch.bar(bar("AAA", 5_000, "1"));
        batch.bar(bar("AAA", 10_000, "2"));
        batch.bar(bar("AAA", 5_000, "3"));

        let closes: Vec<(u64, String)> = batch
            .into_dto(1)
            .bars
            .into_iter()
            .map(|b| (b.time_ms, b.close))
            .collect();
        assert_eq!(closes, vec![(5_000, "3".into()), (10_000, "2".into())]);
    }

    #[test]
    fn an_untouched_batch_is_empty() {
        let mut batch = Batch::default();
        assert!(batch.is_empty());

        batch.account(AccountDto {
            cash: "0".into(),
            positions: vec![],
        });

        assert!(!batch.is_empty());
    }
}
