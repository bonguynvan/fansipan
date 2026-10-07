//! Owns market-data subscriptions and the latest quote per instrument.
//! Adapters stay dumb: they emit, this engine holds state.
//!
//! Subscriptions are reference-counted. Two screens watching the same symbol
//! cost one venue subscription, and closing one of them does not cut the
//! other off. The adapter only hears about the first subscribe and the last
//! unsubscribe.
//!
//! Each subscribed instrument also gets a [`BarSeries`] built from its quote
//! mids, which is what a chart draws, and, with a trade subscription,
//! [`SessionStats`] built from the tape.
//!
//! Reference data (instrument definitions, today's price bands) is kept for
//! every instrument the adapter publishes, subscribed or not: it is small,
//! and screens need it before they subscribe.
//!
//! The engine also holds the market-data connection's status as the adapter
//! last reported it, so a screen can tell a quiet market from a dead feed,
//! and each market's session: the phase of its day and what it takes.

mod bars;
mod session;

use std::collections::{BTreeMap, HashMap};
use std::time::Duration;

use adapter_api::{AdapterError, DataClient};
use bus::BusHandle;
use model::{
    Connection, ConnectionStatus, Depth, InstrumentId, InstrumentInfo, PriceBand, Quote,
    SessionStatus, Trade,
};

pub use bars::{Bar, BarSeries};
pub use session::SessionStats;

pub const DEFAULT_BAR_INTERVAL: Duration = Duration::from_secs(5);
pub const DEFAULT_BAR_CAPACITY: usize = 1_000;

pub struct DataEngine {
    client: Box<dyn DataClient>,
    quote_refs: HashMap<InstrumentId, usize>,
    trade_refs: HashMap<InstrumentId, usize>,
    latest: BTreeMap<InstrumentId, Quote>,
    bars: HashMap<InstrumentId, BarSeries>,
    bar_interval: Duration,
    bar_capacity: usize,
    instruments: BTreeMap<InstrumentId, InstrumentInfo>,
    bands: HashMap<InstrumentId, PriceBand>,
    depth: HashMap<InstrumentId, Depth>,
    stats: HashMap<InstrumentId, SessionStats>,
    sessions: BTreeMap<String, SessionStatus>,
    connection: Option<ConnectionStatus>,
}

/// Takes one reference; true if it is the first, so the adapter must be told.
fn acquire(refs: &mut HashMap<InstrumentId, usize>, instrument: &InstrumentId) -> bool {
    match refs.get_mut(instrument) {
        Some(count) => {
            *count += 1;
            false
        }
        None => true,
    }
}

/// Releases one reference: `None` if there was none, `Some(true)` if it was
/// the last, so the adapter must be told.
fn release(refs: &mut HashMap<InstrumentId, usize>, instrument: &InstrumentId) -> Option<bool> {
    let count = refs.get_mut(instrument)?;
    if *count > 1 {
        *count -= 1;
        return Some(false);
    }
    Some(true)
}

impl DataEngine {
    pub fn new(client: Box<dyn DataClient>) -> Self {
        Self {
            client,
            quote_refs: HashMap::new(),
            trade_refs: HashMap::new(),
            latest: BTreeMap::new(),
            bars: HashMap::new(),
            bar_interval: DEFAULT_BAR_INTERVAL,
            bar_capacity: DEFAULT_BAR_CAPACITY,
            instruments: BTreeMap::new(),
            bands: HashMap::new(),
            depth: HashMap::new(),
            stats: HashMap::new(),
            sessions: BTreeMap::new(),
            connection: None,
        }
    }

    /// Bar interval and how many bars to keep per instrument. Applies to
    /// series created after the call.
    pub fn with_bars(self, interval: Duration, capacity: usize) -> Self {
        Self {
            bar_interval: interval,
            bar_capacity: capacity,
            ..self
        }
    }

    /// A connect that succeeds means up (the contract), so a report from
    /// before it, such as the `Down` of an earlier disconnect, no longer holds.
    pub fn connect(&mut self, bus: Box<dyn BusHandle>) -> Result<(), AdapterError> {
        self.client.connect(bus)?;
        self.connection = None;
        Ok(())
    }

    pub fn subscribe_quotes(&mut self, instrument: &InstrumentId) -> Result<(), AdapterError> {
        if acquire(&mut self.quote_refs, instrument) {
            self.client.subscribe_quotes(instrument)?;
            self.quote_refs.insert(instrument.clone(), 1);
        }
        Ok(())
    }

    /// Releases one reference. Unsubscribing something not subscribed is a
    /// no-op, so a screen can release on close without tracking whether its
    /// subscribe succeeded.
    pub fn unsubscribe_quotes(&mut self, instrument: &InstrumentId) -> Result<(), AdapterError> {
        if release(&mut self.quote_refs, instrument) == Some(true) {
            self.client.unsubscribe_quotes(instrument)?;
            self.quote_refs.remove(instrument);
            self.latest.remove(instrument);
            self.bars.remove(instrument);
            self.depth.remove(instrument);
        }
        Ok(())
    }

    pub fn subscribe_trades(&mut self, instrument: &InstrumentId) -> Result<(), AdapterError> {
        if acquire(&mut self.trade_refs, instrument) {
            self.client.subscribe_trades(instrument)?;
            self.trade_refs.insert(instrument.clone(), 1);
        }
        Ok(())
    }

    pub fn unsubscribe_trades(&mut self, instrument: &InstrumentId) -> Result<(), AdapterError> {
        if release(&mut self.trade_refs, instrument) == Some(true) {
            self.client.unsubscribe_trades(instrument)?;
            self.trade_refs.remove(instrument);
            self.stats.remove(instrument);
        }
        Ok(())
    }

    pub fn is_subscribed(&self, instrument: &InstrumentId) -> bool {
        self.quote_refs.contains_key(instrument)
    }

    /// Caches the quote and folds its mid into the instrument's bars, if
    /// anyone is subscribed to it. Returns whether it was kept, so the caller
    /// knows whether to fan it out. Quotes for unsubscribed instruments are
    /// dropped: an adapter can lag behind an unsubscribe, and the cache must
    /// not grow without bound.
    pub fn on_quote(&mut self, quote: &Quote) -> bool {
        if !self.is_subscribed(&quote.instrument) {
            return false;
        }
        let (interval, capacity) = (self.bar_interval, self.bar_capacity);
        self.bars
            .entry(quote.instrument.clone())
            .or_insert_with(|| BarSeries::new(interval, capacity))
            .on_price(quote.mid(), quote.ts_event);
        self.latest.insert(quote.instrument.clone(), quote.clone());
        true
    }

    /// The bar in progress for an instrument.
    pub fn last_bar(&self, instrument: &InstrumentId) -> Option<&Bar> {
        self.bars.get(instrument).and_then(BarSeries::last)
    }

    /// The newest `limit` bars of an instrument, oldest first.
    pub fn bars(&self, instrument: &InstrumentId, limit: usize) -> Vec<Bar> {
        self.bars
            .get(instrument)
            .map(|series| series.recent(limit).cloned().collect())
            .unwrap_or_default()
    }

    pub fn latest(&self, instrument: &InstrumentId) -> Option<&Quote> {
        self.latest.get(instrument)
    }

    /// Records how an instrument trades. Kept whether or not anyone is
    /// subscribed.
    pub fn on_instrument(&mut self, info: &InstrumentInfo) {
        self.instruments.insert(info.id.clone(), info.clone());
    }

    /// Records today's band. Kept whether or not anyone is subscribed.
    pub fn on_band(&mut self, band: &PriceBand) {
        self.bands.insert(band.instrument.clone(), band.clone());
    }

    /// Caches depth for quote-subscribed instruments; returns whether kept.
    pub fn on_depth(&mut self, depth: &Depth) -> bool {
        if !self.is_subscribed(&depth.instrument) {
            return false;
        }
        self.depth.insert(depth.instrument.clone(), depth.clone());
        true
    }

    /// Folds a print into the session stats of a trade-subscribed instrument;
    /// returns whether kept.
    pub fn on_trade(&mut self, trade: &Trade) -> bool {
        if !self.trade_refs.contains_key(&trade.instrument) {
            return false;
        }
        let stats = self.stats.entry(trade.instrument.clone()).or_default();
        *stats = stats.with_trade(trade.price, trade.quantity);
        true
    }

    pub fn instrument(&self, instrument: &InstrumentId) -> Option<&InstrumentInfo> {
        self.instruments.get(instrument)
    }

    /// Every instrument the adapter has described, ordered by instrument.
    pub fn instruments(&self) -> impl Iterator<Item = &InstrumentInfo> {
        self.instruments.values()
    }

    pub fn band(&self, instrument: &InstrumentId) -> Option<&PriceBand> {
        self.bands.get(instrument)
    }

    pub fn depth(&self, instrument: &InstrumentId) -> Option<&Depth> {
        self.depth.get(instrument)
    }

    pub fn stats(&self, instrument: &InstrumentId) -> Option<&SessionStats> {
        self.stats.get(instrument)
    }

    /// Latest quote per subscribed instrument, ordered by instrument.
    pub fn quotes(&self) -> impl Iterator<Item = &Quote> {
        self.latest.values()
    }

    /// Records a market's current session, replacing its last one.
    pub fn on_session(&mut self, session: &SessionStatus) {
        self.sessions
            .insert(session.market.clone(), session.clone());
    }

    /// Every market's current session, ordered by market.
    pub fn sessions(&self) -> impl Iterator<Item = &SessionStatus> {
        self.sessions.values()
    }

    /// Records a report on the market-data connection; returns whether kept.
    /// Reports on the adapter's other connections are not this engine's.
    pub fn on_connection(&mut self, status: &ConnectionStatus) -> bool {
        if status.connection != Connection::MarketData {
            return false;
        }
        self.connection = Some(status.clone());
        true
    }

    /// The market-data connection as last reported. `None` means no report
    /// since `connect` succeeded, which the contract defines as up.
    pub fn connection(&self) -> Option<&ConnectionStatus> {
        self.connection.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use model::{ConnectionState, Decimal, UnixNanos};
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct Calls {
        subscribed: Vec<InstrumentId>,
        unsubscribed: Vec<InstrumentId>,
        trade_subscribed: Vec<InstrumentId>,
        trade_unsubscribed: Vec<InstrumentId>,
    }

    struct RecordingClient(Arc<Mutex<Calls>>);

    impl DataClient for RecordingClient {
        fn connect(&mut self, _bus: Box<dyn BusHandle>) -> Result<(), AdapterError> {
            Ok(())
        }
        fn subscribe_quotes(&mut self, instrument: &InstrumentId) -> Result<(), AdapterError> {
            if instrument.symbol == "BAD" {
                return Err(AdapterError::fatal("unknown instrument"));
            }
            self.0.lock().unwrap().subscribed.push(instrument.clone());
            Ok(())
        }
        fn unsubscribe_quotes(&mut self, instrument: &InstrumentId) -> Result<(), AdapterError> {
            self.0.lock().unwrap().unsubscribed.push(instrument.clone());
            Ok(())
        }
        fn subscribe_trades(&mut self, instrument: &InstrumentId) -> Result<(), AdapterError> {
            if instrument.symbol == "BAD" {
                return Err(AdapterError::fatal("unknown instrument"));
            }
            self.0
                .lock()
                .unwrap()
                .trade_subscribed
                .push(instrument.clone());
            Ok(())
        }
        fn unsubscribe_trades(&mut self, instrument: &InstrumentId) -> Result<(), AdapterError> {
            self.0
                .lock()
                .unwrap()
                .trade_unsubscribed
                .push(instrument.clone());
            Ok(())
        }
        fn disconnect(&mut self) -> Result<(), AdapterError> {
            Ok(())
        }
    }

    fn engine() -> (DataEngine, Arc<Mutex<Calls>>) {
        let calls = Arc::new(Mutex::new(Calls::default()));
        (
            DataEngine::new(Box::new(RecordingClient(calls.clone()))),
            calls,
        )
    }

    fn id(symbol: &str) -> InstrumentId {
        InstrumentId::new("sim", symbol)
    }

    fn quote(symbol: &str, bid: i64) -> Quote {
        quote_at(symbol, bid, 0)
    }

    fn quote_at(symbol: &str, bid: i64, seconds: u64) -> Quote {
        Quote {
            instrument: id(symbol),
            bid: Decimal::from(bid),
            ask: Decimal::from(bid + 2),
            bid_size: Decimal::ONE,
            ask_size: Decimal::ONE,
            ts_event: UnixNanos(seconds * 1_000_000_000),
        }
    }

    fn trade(symbol: &str, price: i64, quantity: i64) -> Trade {
        Trade {
            instrument: id(symbol),
            price: Decimal::from(price),
            quantity: Decimal::from(quantity),
            side: Some(model::Side::Buy),
            ts_event: UnixNanos(0),
        }
    }

    #[test]
    fn trades_build_session_stats_once_subscribed() {
        let (mut engine, calls) = engine();
        assert!(!engine.on_trade(&trade("AAA", 10, 100)));

        engine.subscribe_trades(&id("AAA")).unwrap();
        engine.subscribe_trades(&id("AAA")).unwrap();
        engine.on_trade(&trade("AAA", 10, 100));
        engine.on_trade(&trade("AAA", 12, 300));

        let stats = engine.stats(&id("AAA")).unwrap();
        assert_eq!(stats.volume, Decimal::from(400));
        assert_eq!(stats.high, Some(Decimal::from(12)));
        assert_eq!(calls.lock().unwrap().trade_subscribed.len(), 1);
    }

    #[test]
    fn the_last_trade_unsubscribe_drops_the_stats() {
        let (mut engine, _) = engine();
        engine.subscribe_trades(&id("AAA")).unwrap();
        engine.on_trade(&trade("AAA", 10, 100));

        engine.unsubscribe_trades(&id("AAA")).unwrap();

        assert!(engine.stats(&id("AAA")).is_none());
    }

    #[test]
    fn reference_data_is_kept_without_a_subscription() {
        let (mut engine, _) = engine();
        let ticks = model::TickTable::flat(Decimal::ONE).unwrap();

        engine.on_instrument(&InstrumentInfo::new(id("AAA"), Decimal::ONE_HUNDRED, ticks));
        engine.on_band(&PriceBand::new(
            id("AAA"),
            Decimal::TEN,
            None,
            None,
            UnixNanos(0),
        ));

        assert_eq!(engine.instruments().count(), 1);
        assert_eq!(
            engine.band(&id("AAA")).map(|b| b.reference),
            Some(Decimal::TEN)
        );
    }

    #[test]
    fn depth_is_kept_only_for_quote_subscribed_instruments() {
        let (mut engine, _) = engine();
        let depth = Depth::new(id("AAA"), vec![], vec![], UnixNanos(0));
        assert!(!engine.on_depth(&depth));

        engine.subscribe_quotes(&id("AAA")).unwrap();

        assert!(engine.on_depth(&depth));
        assert!(engine.depth(&id("AAA")).is_some());
    }

    #[test]
    fn quotes_build_bars_from_the_mid() {
        let (mut engine, _) = engine();
        engine.subscribe_quotes(&id("AAA")).unwrap();

        engine.on_quote(&quote_at("AAA", 10, 100));
        engine.on_quote(&quote_at("AAA", 14, 101));
        engine.on_quote(&quote_at("AAA", 12, 106));

        let bars = engine.bars(&id("AAA"), 10);
        assert_eq!(bars.len(), 2);
        assert_eq!(
            (bars[0].open, bars[0].close),
            (Decimal::from(11), Decimal::from(15))
        );
        assert_eq!(
            engine.last_bar(&id("AAA")).map(|b| b.close),
            Some(Decimal::from(13))
        );
    }

    #[test]
    fn dropped_quotes_build_no_bars() {
        let (mut engine, _) = engine();

        engine.on_quote(&quote_at("AAA", 10, 100));

        assert!(engine.bars(&id("AAA"), 10).is_empty());
    }

    #[test]
    fn last_unsubscribe_drops_the_bars() {
        let (mut engine, _) = engine();
        engine.subscribe_quotes(&id("AAA")).unwrap();
        engine.on_quote(&quote_at("AAA", 10, 100));

        engine.unsubscribe_quotes(&id("AAA")).unwrap();

        assert!(engine.last_bar(&id("AAA")).is_none());
    }

    #[test]
    fn bar_interval_and_capacity_are_configurable() {
        let (engine, _) = engine();
        let mut engine = engine.with_bars(std::time::Duration::from_secs(60), 2);
        engine.subscribe_quotes(&id("AAA")).unwrap();

        for minute in 0..3 {
            engine.on_quote(&quote_at("AAA", 10, 100 + minute * 60));
        }

        assert_eq!(engine.bars(&id("AAA"), 10).len(), 2);
    }

    #[test]
    fn keeps_only_the_latest_quote_per_instrument() {
        let (mut engine, _) = engine();
        engine.subscribe_quotes(&id("AAA")).unwrap();

        engine.on_quote(&quote("AAA", 10));
        engine.on_quote(&quote("AAA", 11));

        assert_eq!(
            engine.latest(&id("AAA")).map(|q| q.bid),
            Some(Decimal::from(11))
        );
        assert_eq!(engine.quotes().count(), 1);
    }

    #[test]
    fn drops_quotes_for_instruments_nobody_subscribed_to() {
        let (mut engine, _) = engine();

        let kept = engine.on_quote(&quote("AAA", 10));

        assert!(!kept);
        assert_eq!(engine.quotes().count(), 0);
    }

    #[test]
    fn the_adapter_hears_only_the_first_subscribe_and_the_last_unsubscribe() {
        let (mut engine, calls) = engine();

        engine.subscribe_quotes(&id("AAA")).unwrap();
        engine.subscribe_quotes(&id("AAA")).unwrap();
        engine.unsubscribe_quotes(&id("AAA")).unwrap();

        assert_eq!(calls.lock().unwrap().subscribed.len(), 1);
        assert!(calls.lock().unwrap().unsubscribed.is_empty());
        assert!(engine.is_subscribed(&id("AAA")));

        engine.unsubscribe_quotes(&id("AAA")).unwrap();

        assert_eq!(calls.lock().unwrap().unsubscribed.len(), 1);
        assert!(!engine.is_subscribed(&id("AAA")));
    }

    #[test]
    fn last_unsubscribe_clears_the_cached_quote() {
        let (mut engine, _) = engine();
        engine.subscribe_quotes(&id("AAA")).unwrap();
        engine.on_quote(&quote("AAA", 10));

        engine.unsubscribe_quotes(&id("AAA")).unwrap();

        assert!(engine.latest(&id("AAA")).is_none());
    }

    #[test]
    fn a_failed_subscribe_is_not_counted() {
        let (mut engine, _) = engine();

        assert!(engine.subscribe_quotes(&id("BAD")).is_err());

        assert!(!engine.is_subscribed(&id("BAD")));
    }

    #[test]
    fn a_failed_trade_subscribe_is_not_counted() {
        let (mut engine, calls) = engine();

        assert!(engine.subscribe_trades(&id("BAD")).is_err());
        engine.unsubscribe_trades(&id("BAD")).unwrap();

        assert!(calls.lock().unwrap().trade_unsubscribed.is_empty());
    }

    #[test]
    fn trade_stats_survive_until_the_last_trade_subscriber_leaves() {
        let (mut engine, calls) = engine();
        engine.subscribe_trades(&id("AAA")).unwrap();
        engine.subscribe_trades(&id("AAA")).unwrap();
        engine.on_trade(&trade("AAA", 10, 5));

        engine.unsubscribe_trades(&id("AAA")).unwrap();
        let kept = engine.stats(&id("AAA")).map(|s| s.volume);
        engine.unsubscribe_trades(&id("AAA")).unwrap();

        assert_eq!(kept, Some(Decimal::from(5)));
        assert!(engine.stats(&id("AAA")).is_none());
        let calls = calls.lock().unwrap();
        assert_eq!(calls.trade_subscribed, vec![id("AAA")]);
        assert_eq!(calls.trade_unsubscribed, vec![id("AAA")]);
    }

    #[test]
    fn unsubscribing_something_never_subscribed_is_a_no_op() {
        let (mut engine, calls) = engine();

        engine.unsubscribe_quotes(&id("AAA")).unwrap();

        assert!(calls.lock().unwrap().unsubscribed.is_empty());
    }

    #[test]
    fn keeps_the_latest_market_data_connection_report_and_no_other() {
        let (mut engine, _) = engine();
        assert_eq!(engine.connection(), None);
        let lost = ConnectionStatus::new(
            Connection::MarketData,
            ConnectionState::Reconnecting,
            UnixNanos(1),
        )
        .with_detail("stream closed");
        let back = ConnectionStatus::new(Connection::MarketData, ConnectionState::Up, UnixNanos(2));
        let orders = ConnectionStatus::new(Connection::Orders, ConnectionState::Down, UnixNanos(3));

        assert!(engine.on_connection(&lost));
        assert!(engine.on_connection(&back));
        assert!(!engine.on_connection(&orders));

        assert_eq!(engine.connection(), Some(&back));
    }

    #[test]
    fn connecting_again_forgets_the_last_report() {
        let (mut engine, _) = engine();
        let down =
            ConnectionStatus::new(Connection::MarketData, ConnectionState::Down, UnixNanos(1));
        engine.on_connection(&down);

        engine.connect(bus::Bus::new().handle()).unwrap();

        assert_eq!(engine.connection(), None);
    }

    #[test]
    fn keeps_each_markets_latest_session() {
        let (mut engine, _) = engine();
        let open = SessionStatus::new("HOSE", model::SessionPhase::Continuous, UnixNanos(1));
        let closing = SessionStatus::new("HOSE", model::SessionPhase::ClosingAuction, UnixNanos(2));
        let hnx = SessionStatus::new("HNX", model::SessionPhase::Continuous, UnixNanos(2));

        engine.on_session(&open);
        engine.on_session(&closing);
        engine.on_session(&hnx);

        let markets: Vec<(&str, model::SessionPhase)> = engine
            .sessions()
            .map(|s| (s.market.as_str(), s.phase))
            .collect();
        assert_eq!(
            markets,
            vec![
                ("HNX", model::SessionPhase::Continuous),
                ("HOSE", model::SessionPhase::ClosingAuction)
            ]
        );
    }
}
