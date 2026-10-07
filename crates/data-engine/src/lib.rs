//! Owns market-data subscriptions and the latest quote per instrument.
//! Adapters stay dumb: they emit, this engine holds state.
//!
//! Subscriptions are reference-counted. Two screens watching the same symbol
//! cost one venue subscription, and closing one of them does not cut the
//! other off. The adapter only hears about the first subscribe and the last
//! unsubscribe.
//!
//! Each subscribed instrument also gets a [`BarSeries`] built from its quote
//! mids, which is what a chart draws.

mod bars;

use std::collections::{BTreeMap, HashMap};
use std::time::Duration;

use adapter_api::{AdapterError, DataClient};
use bus::BusHandle;
use model::{InstrumentId, Quote};

pub use bars::{Bar, BarSeries};

pub const DEFAULT_BAR_INTERVAL: Duration = Duration::from_secs(5);
pub const DEFAULT_BAR_CAPACITY: usize = 1_000;

pub struct DataEngine {
    client: Box<dyn DataClient>,
    quote_refs: HashMap<InstrumentId, usize>,
    latest: BTreeMap<InstrumentId, Quote>,
    bars: HashMap<InstrumentId, BarSeries>,
    bar_interval: Duration,
    bar_capacity: usize,
}

impl DataEngine {
    pub fn new(client: Box<dyn DataClient>) -> Self {
        Self {
            client,
            quote_refs: HashMap::new(),
            latest: BTreeMap::new(),
            bars: HashMap::new(),
            bar_interval: DEFAULT_BAR_INTERVAL,
            bar_capacity: DEFAULT_BAR_CAPACITY,
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

    pub fn connect(&mut self, bus: Box<dyn BusHandle>) -> Result<(), AdapterError> {
        self.client.connect(bus)
    }

    pub fn subscribe_quotes(&mut self, instrument: &InstrumentId) -> Result<(), AdapterError> {
        if let Some(count) = self.quote_refs.get_mut(instrument) {
            *count += 1;
            return Ok(());
        }
        self.client.subscribe_quotes(instrument)?;
        self.quote_refs.insert(instrument.clone(), 1);
        Ok(())
    }

    /// Releases one reference. Unsubscribing something not subscribed is a
    /// no-op, so a screen can release on close without tracking whether its
    /// subscribe succeeded.
    pub fn unsubscribe_quotes(&mut self, instrument: &InstrumentId) -> Result<(), AdapterError> {
        match self.quote_refs.get(instrument).copied() {
            None => Ok(()),
            Some(1) => {
                self.client.unsubscribe_quotes(instrument)?;
                self.quote_refs.remove(instrument);
                self.latest.remove(instrument);
                self.bars.remove(instrument);
                Ok(())
            }
            Some(count) => {
                self.quote_refs.insert(instrument.clone(), count - 1);
                Ok(())
            }
        }
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

    /// Latest quote per subscribed instrument, ordered by instrument.
    pub fn quotes(&self) -> impl Iterator<Item = &Quote> {
        self.latest.values()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use model::{Decimal, UnixNanos};
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct Calls {
        subscribed: Vec<InstrumentId>,
        unsubscribed: Vec<InstrumentId>,
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
        fn subscribe_trades(&mut self, _instrument: &InstrumentId) -> Result<(), AdapterError> {
            Ok(())
        }
        fn unsubscribe_trades(&mut self, _instrument: &InstrumentId) -> Result<(), AdapterError> {
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
    fn unsubscribing_something_never_subscribed_is_a_no_op() {
        let (mut engine, calls) = engine();

        engine.unsubscribe_quotes(&id("AAA")).unwrap();

        assert!(calls.lock().unwrap().unsubscribed.is_empty());
    }
}
