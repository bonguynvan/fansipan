//! Conflates engine output into one message per flush for the front end.
//!
//! A feed can produce thousands of events a second; a screen shows one state
//! per frame. Between flushes only the latest quote, board row, and
//! instrument per symbol, the latest state per order and per connection
//! survive, and each changed bar is kept once, so the webview receives a
//! bounded number of messages whatever the feed rate.
//!
//! Prints are not conflated, since time and sales shows each one. A symbol's
//! prints in one batch are capped at a default tape's size: a burst bigger
//! than that in one flush is more than any reader could scroll back
//! through.

use std::collections::{BTreeMap, VecDeque};

use data_engine::DEFAULT_TAPE_CAPACITY;
use serde::Serialize;

use crate::dto::{
    AccountDto, BarDto, BoardDto, ConditionalDto, ConnectionDto, DayDto, InstrumentDto,
    InstrumentStatusDto, OrderDto, PrintDto, QuoteDto, SessionDto,
};

#[derive(Default)]
pub struct Batch {
    quotes: BTreeMap<String, QuoteDto>,
    orders: BTreeMap<u64, OrderDto>,
    account: Option<AccountDto>,
    bars: BTreeMap<(String, u64), BarDto>,
    instruments: BTreeMap<String, InstrumentDto>,
    boards: BTreeMap<String, BoardDto>,
    connections: BTreeMap<&'static str, ConnectionDto>,
    sessions: BTreeMap<String, SessionDto>,
    statuses: BTreeMap<String, InstrumentStatusDto>,
    prints: BTreeMap<String, VecDeque<PrintDto>>,
    trading: Option<&'static str>,
    conditionals: BTreeMap<u64, ConditionalDto>,
    journal_error: Option<String>,
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
    pub instruments: Vec<InstrumentDto>,
    pub boards: Vec<BoardDto>,
    pub connections: Vec<ConnectionDto>,
    pub sessions: Vec<SessionDto>,
    /// Instruments whose status changed (ADR 0026).
    pub statuses: Vec<InstrumentStatusDto>,
    /// Every print since the last batch, by symbol, then oldest first.
    pub prints: Vec<PrintDto>,
    /// The trading state, when it changed since the last batch.
    pub trading: Option<&'static str>,
    /// The day's profit and loss as of this batch.
    pub day: Option<DayDto>,
    /// Conditional orders that changed: placed, moved, fired, or cancelled.
    pub conditionals: Vec<ConditionalDto>,
    /// Why the journal could not be written, the first time it fails.
    pub journal_error: Option<String>,
}

impl Batch {
    /// The ids of the orders and conditional orders this batch carries, and
    /// whether the trader's switch moved: what the journal records.
    pub fn changed(&self) -> (Vec<u64>, Vec<u64>, bool) {
        (
            self.orders.keys().copied().collect(),
            self.conditionals.keys().copied().collect(),
            self.trading.is_some(),
        )
    }

    pub fn conditional(&mut self, conditional: ConditionalDto) {
        self.conditionals.insert(conditional.id, conditional);
    }

    /// A journal problem a command met, for the screens.
    pub fn journal_error(&mut self, problem: String) {
        self.journal_error = Some(problem);
    }

    pub fn trading(&mut self, state: &'static str) {
        self.trading = Some(state);
    }

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

    pub fn instrument(&mut self, instrument: InstrumentDto) {
        self.instruments
            .insert(instrument.symbol.clone(), instrument);
    }

    pub fn board(&mut self, board: BoardDto) {
        self.boards.insert(board.symbol.clone(), board);
    }

    pub fn connection(&mut self, connection: ConnectionDto) {
        self.connections.insert(connection.connection, connection);
    }

    pub fn session(&mut self, session: SessionDto) {
        self.sessions.insert(session.market.clone(), session);
    }

    pub fn instrument_status(&mut self, status: InstrumentStatusDto) {
        self.statuses.insert(status.symbol.clone(), status);
    }

    pub fn print(&mut self, print: PrintDto) {
        let tape = self.prints.entry(print.symbol.clone()).or_default();
        if tape.len() == DEFAULT_TAPE_CAPACITY {
            tape.pop_front();
        }
        tape.push_back(print);
    }

    pub fn is_empty(&self) -> bool {
        self.quotes.is_empty()
            && self.orders.is_empty()
            && self.account.is_none()
            && self.bars.is_empty()
            && self.instruments.is_empty()
            && self.boards.is_empty()
            && self.connections.is_empty()
            && self.sessions.is_empty()
            && self.statuses.is_empty()
            && self.prints.is_empty()
            && self.trading.is_none()
            && self.conditionals.is_empty()
            && self.journal_error.is_none()
    }

    pub fn into_dto(self, seq: u64) -> BatchDto {
        BatchDto {
            seq,
            quotes: self.quotes.into_values().collect(),
            orders: self.orders.into_values().collect(),
            account: self.account,
            bars: self.bars.into_values().collect(),
            instruments: self.instruments.into_values().collect(),
            boards: self.boards.into_values().collect(),
            connections: self.connections.into_values().collect(),
            sessions: self.sessions.into_values().collect(),
            statuses: self.statuses.into_values().collect(),
            prints: self.prints.into_values().flatten().collect(),
            trading: self.trading,
            day: None,
            conditionals: self.conditionals.into_values().collect(),
            journal_error: self.journal_error,
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

    fn board(symbol: &str, last: &str) -> BoardDto {
        BoardDto {
            symbol: symbol.into(),
            exchange: String::new(),
            reference: None,
            ceiling: None,
            floor: None,
            bids: vec![],
            asks: vec![],
            last: Some(last.into()),
            last_quantity: None,
            change: None,
            change_pct: None,
            volume: "0".into(),
            high: None,
            low: None,
        }
    }

    #[test]
    fn only_the_latest_board_row_per_symbol_survives() {
        let mut batch = Batch::default();

        batch.board(board("VNM", "62500"));
        batch.board(board("VNM", "62600"));

        let dto = batch.into_dto(1);
        assert_eq!(dto.boards.len(), 1);
        assert_eq!(dto.boards[0].last.as_deref(), Some("62600"));
    }

    fn print(symbol: &str, seq: u64) -> PrintDto {
        PrintDto {
            symbol: symbol.into(),
            seq,
            price: "10".into(),
            quantity: "100".into(),
            side: None,
            time_ms: 0,
        }
    }

    #[test]
    fn every_print_is_kept_by_symbol_in_tape_order() {
        let mut batch = Batch::default();

        batch.print(print("BBB", 7));
        batch.print(print("AAA", 1));
        batch.print(print("BBB", 8));

        let prints: Vec<(String, u64)> = batch
            .into_dto(1)
            .prints
            .into_iter()
            .map(|p| (p.symbol, p.seq))
            .collect();
        assert_eq!(
            prints,
            vec![("AAA".into(), 1), ("BBB".into(), 7), ("BBB".into(), 8)]
        );
    }

    #[test]
    fn a_batch_holds_no_more_of_a_symbols_prints_than_its_tape() {
        let mut batch = Batch::default();

        for seq in 1..=(DEFAULT_TAPE_CAPACITY as u64 + 2) {
            batch.print(print("AAA", seq));
        }

        let prints = batch.into_dto(1).prints;
        assert_eq!(prints.len(), DEFAULT_TAPE_CAPACITY);
        assert_eq!(prints[0].seq, 3);
    }

    #[test]
    fn an_untouched_batch_is_empty() {
        let mut batch = Batch::default();
        assert!(batch.is_empty());

        batch.account(AccountDto {
            cash: "0".into(),
            positions: vec![],
            buying_power: None,
        });

        assert!(!batch.is_empty());
    }
}
