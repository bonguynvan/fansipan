//! Simulated venue for demos, CI, and development. Not a real venue: no
//! network, no protocol. A random walk moves prices along each listing's
//! tick ladder and inside its price band; market orders sweep, limit orders
//! rest and fill when the walk reaches them, at most the displayed size per
//! order per price move.
//!
//! The two clients are thin: [`SimVenue`] does the matching and the clients
//! only move its events onto the bus, which is the shape a real adapter has
//! too (decode, translate, publish). [`MarketProfile`] supplies the
//! listings and their market rules.

pub mod day;
pub mod profiles;
mod venue;

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use adapter_api::{AdapterError, DataClient, ExecutionClient};
use bus::{BusHandle, Event};
use model::{
    AccountState, CancelOrder, Connection, ConnectionState, ConnectionStatus, InstrumentId,
    SubmitOrder, UnixNanos,
};

pub use day::{Clock, MarketDay, Phase, TradingDay};
pub use profiles::MarketProfile;
pub use venue::{Listing, SimVenue, Tick, DEPTH_LEVELS, MIN_MID, TICK, TOP_OF_BOOK_SIZE};

const TICK_INTERVAL: Duration = Duration::from_millis(500);
const WALK_SEED: u64 = 0x9E37_79B9_7F4A_7C15;
/// Quotes of made-up history sent when an instrument is first subscribed:
/// twenty minutes at the beat's pace, so a chart opens with something to
/// show instead of filling up one bar at a time.
const HISTORY_QUOTES: usize = 2_400;

type Subscriptions = Arc<Mutex<HashSet<InstrumentId>>>;

fn locked(set: &Subscriptions) -> MutexGuard<'_, HashSet<InstrumentId>> {
    set.lock().unwrap_or_else(PoisonError::into_inner)
}

pub struct SandboxDataClient {
    venue: SimVenue,
    quotes: Subscriptions,
    trades: Subscriptions,
    running: Arc<AtomicBool>,
}

impl SandboxDataClient {
    pub fn new(venue: SimVenue) -> Self {
        Self {
            venue,
            quotes: Arc::default(),
            trades: Arc::default(),
            running: Arc::default(),
        }
    }

    fn listed(&self, instrument: &InstrumentId) -> Result<(), AdapterError> {
        if self.venue.is_listed(instrument) {
            Ok(())
        } else {
            Err(AdapterError::fatal(format!(
                "sandbox: {instrument} is not listed"
            )))
        }
    }
}

impl DataClient for SandboxDataClient {
    fn connect(&mut self, bus: Box<dyn BusHandle>) -> Result<(), AdapterError> {
        if self.running.swap(true, Ordering::SeqCst) {
            return Err(AdapterError::fatal(
                "sandbox: data client already connected",
            ));
        }
        let running = self.running.clone();
        let venue = self.venue.clone();
        let quotes = self.quotes.clone();
        let trades = self.trades.clone();
        thread::Builder::new()
            .name("sandbox-market".into())
            .spawn(move || {
                let mut state = BeatState::default();
                while running.load(Ordering::SeqCst) {
                    let quoted = locked(&quotes).clone();
                    let printed = locked(&trades).clone();
                    let beat = Beat {
                        venue: &venue,
                        quotes: &quoted,
                        trades: &printed,
                        bus: bus.as_ref(),
                    };
                    beat.run(&mut state, now());
                    thread::sleep(TICK_INTERVAL);
                }
                bus.publish(down(Connection::MarketData));
            })
            .map(drop)
            .map_err(|e| {
                self.running.store(false, Ordering::SeqCst);
                AdapterError::fatal(format!("sandbox: cannot start market thread: {e}"))
            })
    }

    fn subscribe_quotes(&mut self, instrument: &InstrumentId) -> Result<(), AdapterError> {
        self.listed(instrument)?;
        locked(&self.quotes).insert(instrument.clone());
        Ok(())
    }

    fn unsubscribe_quotes(&mut self, instrument: &InstrumentId) -> Result<(), AdapterError> {
        locked(&self.quotes).remove(instrument);
        Ok(())
    }

    fn subscribe_trades(&mut self, instrument: &InstrumentId) -> Result<(), AdapterError> {
        self.listed(instrument)?;
        locked(&self.trades).insert(instrument.clone());
        Ok(())
    }

    fn unsubscribe_trades(&mut self, instrument: &InstrumentId) -> Result<(), AdapterError> {
        locked(&self.trades).remove(instrument);
        Ok(())
    }

    fn disconnect(&mut self) -> Result<(), AdapterError> {
        self.running.store(false, Ordering::SeqCst);
        Ok(())
    }
}

/// What the market thread remembers between beats.
struct BeatState {
    /// Reference data goes out once, before the first quote.
    announced: bool,
    backfilled: HashSet<InstrumentId>,
    seed: u64,
}

impl Default for BeatState {
    fn default() -> Self {
        Self {
            announced: false,
            backfilled: HashSet::new(),
            seed: WALK_SEED,
        }
    }
}

/// What one beat of the simulated market reads.
struct Beat<'a> {
    venue: &'a SimVenue,
    quotes: &'a HashSet<InstrumentId>,
    trades: &'a HashSet<InstrumentId>,
    bus: &'a dyn BusHandle,
}

impl Beat<'_> {
    /// The first beat announces every listing's rules, today's band, and
    /// each market's session, so a connection that comes after another
    /// learns them again. Every beat then moves each market to its phase of
    /// the day, which publishes its session when it changes. Newly quote-subscribed
    /// instruments get their made-up history. Then every listed instrument
    /// in continuous trading takes a random step: quotes and depth go out
    /// for quote subscriptions, prints for trade subscriptions, fills for
    /// all. Unsubscribing forgets the backfill, so subscribing again brings
    /// a fresh history.
    fn run(&self, state: &mut BeatState, now: UnixNanos) {
        if !state.announced {
            state.announced = true;
            self.venue
                .reference_data(now)
                .into_iter()
                .chain(self.venue.sessions(now))
                .for_each(|event| self.bus.publish(event));
        }
        self.venue
            .advance(now, &mut |event| self.bus.publish(event));

        state
            .backfilled
            .retain(|instrument| self.quotes.contains(instrument));
        for instrument in self.quotes {
            if state.backfilled.insert(instrument.clone()) {
                self.venue
                    .synthetic_history(
                        instrument,
                        now,
                        HISTORY_QUOTES,
                        TICK_INTERVAL,
                        &mut state.seed,
                    )
                    .into_iter()
                    .for_each(|quote| self.bus.publish(Event::Quote(quote)));
            }
        }

        for instrument in self.venue.instruments() {
            let publish = &mut |event| self.bus.publish(event);
            let Some(tick) = self.venue.walk(&instrument, now, &mut state.seed, publish) else {
                continue;
            };
            if self.quotes.contains(&instrument) {
                self.bus.publish(Event::Quote(tick.quote));
                self.bus.publish(Event::Depth(tick.depth));
            }
            if let Some(trade) = tick.trade.filter(|_| self.trades.contains(&instrument)) {
                self.bus.publish(Event::Trade(trade));
            }
        }
    }
}

pub struct SandboxExecutionClient {
    venue: SimVenue,
    bus: Option<Box<dyn BusHandle>>,
}

impl SandboxExecutionClient {
    pub fn new(venue: SimVenue) -> Self {
        Self { venue, bus: None }
    }

    fn bus(&self) -> Result<&dyn BusHandle, AdapterError> {
        self.bus.as_deref().ok_or_else(not_connected)
    }
}

impl ExecutionClient for SandboxExecutionClient {
    fn connect(&mut self, bus: Box<dyn BusHandle>) -> Result<(), AdapterError> {
        bus.publish(Event::AccountUpdate(self.venue.account()));
        self.bus = Some(bus);
        Ok(())
    }

    fn submit_order(&mut self, order: SubmitOrder) -> Result<(), AdapterError> {
        let bus = self.bus()?;
        self.venue.submit(order, now(), &mut |e| bus.publish(e));
        Ok(())
    }

    fn cancel_order(&mut self, cancel: CancelOrder) -> Result<(), AdapterError> {
        let bus = self.bus()?;
        self.venue.cancel(&cancel, now(), &mut |e| bus.publish(e));
        Ok(())
    }

    fn account_state(&mut self) -> Result<AccountState, AdapterError> {
        Ok(self.venue.account())
    }

    fn disconnect(&mut self) -> Result<(), AdapterError> {
        if let Some(bus) = self.bus.take() {
            bus.publish(down(Connection::Orders));
        }
        Ok(())
    }
}

fn now() -> UnixNanos {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_nanos());
    UnixNanos(u64::try_from(nanos).unwrap_or(u64::MAX))
}

/// A disconnect core asked for: down, and not coming back on its own.
fn down(connection: Connection) -> Event {
    Event::Connection(ConnectionStatus::new(
        connection,
        ConnectionState::Down,
        now(),
    ))
}

fn not_connected() -> AdapterError {
    AdapterError::fatal("sandbox: not connected")
}

#[cfg(test)]
mod tests {
    use super::*;
    use model::{ClientOrderId, Decimal, NewOrder, Side};
    use std::sync::mpsc;
    use std::time::Instant;

    struct ChannelBus(mpsc::Sender<Event>);

    impl BusHandle for ChannelBus {
        fn publish(&self, event: Event) {
            let _ = self.0.send(event);
        }
    }

    fn listed(symbols: &[&str]) -> SimVenue {
        let venue = SimVenue::new(Decimal::from(1_000));
        for symbol in symbols {
            venue.list(InstrumentId::new("sim", *symbol), Decimal::from(10));
        }
        venue
    }

    fn quoted_symbols(rx: &mpsc::Receiver<Event>) -> Vec<String> {
        rx.try_iter()
            .filter_map(|e| match e {
                Event::Quote(q) => Some(q.instrument.symbol),
                _ => None,
            })
            .collect()
    }

    fn ids(symbols: &[&str]) -> HashSet<InstrumentId> {
        symbols
            .iter()
            .map(|s| InstrumentId::new("sim", *s))
            .collect()
    }

    /// A state that has already announced and backfilled `quoted`.
    fn settled(quoted: &HashSet<InstrumentId>) -> BeatState {
        BeatState {
            announced: true,
            backfilled: quoted.clone(),
            seed: 7,
        }
    }

    #[test]
    fn a_beat_publishes_quotes_and_depth_only_for_quote_subscriptions() {
        let venue = listed(&["AAA", "BBB"]);
        let (tx, rx) = mpsc::channel();
        let bus = ChannelBus(tx);
        let quoted = ids(&["AAA"]);
        let beat = Beat {
            venue: &venue,
            quotes: &quoted,
            trades: &HashSet::new(),
            bus: &bus,
        };

        beat.run(&mut settled(&quoted), UnixNanos(1));

        let events: Vec<Event> = rx.try_iter().collect();
        let symbols: Vec<&str> = events
            .iter()
            .filter_map(|e| match e {
                Event::Quote(q) => Some(q.instrument.symbol.as_str()),
                Event::Depth(d) => Some(d.instrument.symbol.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(symbols, vec!["AAA", "AAA"]);
        assert!(!events.iter().any(|e| matches!(e, Event::Trade(_))));
    }

    #[test]
    fn prints_go_out_only_for_trade_subscriptions_and_only_when_price_moves() {
        let venue = listed(&["AAA", "BBB"]);
        let (tx, rx) = mpsc::channel();
        let bus = ChannelBus(tx);
        let printed = ids(&["BBB"]);
        let beat = Beat {
            venue: &venue,
            quotes: &HashSet::new(),
            trades: &printed,
            bus: &bus,
        };
        let mut state = settled(&HashSet::new());

        for _ in 0..20 {
            beat.run(&mut state, UnixNanos(1));
        }

        let traded: HashSet<String> = rx
            .try_iter()
            .filter_map(|e| match e {
                Event::Trade(t) => Some(t.instrument.symbol),
                _ => None,
            })
            .collect();
        assert_eq!(traded, HashSet::from(["BBB".to_string()]));
    }

    #[test]
    fn reference_data_is_announced_once_before_anything_else() {
        let venue = listed(&["AAA", "BBB"]);
        let (tx, rx) = mpsc::channel();
        let bus = ChannelBus(tx);
        let beat = Beat {
            venue: &venue,
            quotes: &HashSet::new(),
            trades: &HashSet::new(),
            bus: &bus,
        };
        let mut state = BeatState::default();

        beat.run(&mut state, UnixNanos(1));
        beat.run(&mut state, UnixNanos(2));

        let announced = rx
            .try_iter()
            .filter(|e| matches!(e, Event::Instrument(_) | Event::PriceBand(_)))
            .count();
        assert_eq!(announced, 4);
    }

    #[test]
    fn a_new_subscription_is_backfilled_once() {
        let venue = listed(&["AAA"]);
        let (tx, rx) = mpsc::channel();
        let bus = ChannelBus(tx);
        let quoted = ids(&["AAA"]);
        let beat = Beat {
            venue: &venue,
            quotes: &quoted,
            trades: &HashSet::new(),
            bus: &bus,
        };
        let mut state = settled(&HashSet::new());
        let now = UnixNanos(10_000_000_000_000);

        beat.run(&mut state, now);
        let first = quoted_symbols(&rx).len();
        beat.run(&mut state, now);
        let second = quoted_symbols(&rx).len();

        assert_eq!(first, HISTORY_QUOTES + 1);
        assert_eq!(second, 1);
    }

    #[test]
    fn an_unsubscribed_instrument_is_forgotten_so_it_backfills_again() {
        let venue = listed(&["AAA"]);
        let (tx, rx) = mpsc::channel();
        let bus = ChannelBus(tx);
        let nobody = HashSet::new();
        let idle = Beat {
            venue: &venue,
            quotes: &nobody,
            trades: &nobody,
            bus: &bus,
        };
        let mut state = settled(&ids(&["AAA"]));

        idle.run(&mut state, UnixNanos(10_000_000_000_000));

        assert!(state.backfilled.is_empty());
        assert!(quoted_symbols(&rx).is_empty());
    }

    #[test]
    fn subscribing_to_an_unlisted_instrument_fails() {
        let mut client = SandboxDataClient::new(listed(&["AAA"]));

        let result = client.subscribe_quotes(&InstrumentId::new("sim", "ZZZ"));

        assert!(result.is_err());
    }

    #[test]
    fn execution_client_refuses_orders_before_connect() {
        let mut client = SandboxExecutionClient::new(listed(&["AAA"]));

        let result = client.submit_order(SubmitOrder::new(
            ClientOrderId(1),
            NewOrder::market(InstrumentId::new("sim", "AAA"), Side::Buy, Decimal::ONE),
        ));

        assert_eq!(result, Err(not_connected()));
    }

    #[test]
    fn connect_publishes_the_opening_account_state() {
        let mut client = SandboxExecutionClient::new(listed(&["AAA"]));
        let (tx, rx) = mpsc::channel();

        client.connect(Box::new(ChannelBus(tx))).unwrap();

        assert!(
            matches!(rx.try_recv(), Ok(Event::AccountUpdate(a)) if a.cash == Decimal::from(1_000))
        );
    }

    fn connection_reports(
        events: impl Iterator<Item = Event>,
    ) -> Vec<(Connection, ConnectionState)> {
        events
            .filter_map(|e| match e {
                Event::Connection(status) => Some((status.connection, status.state)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn disconnecting_the_execution_client_reports_its_connection_down() {
        let mut client = SandboxExecutionClient::new(listed(&["AAA"]));
        let (tx, rx) = mpsc::channel();
        client.connect(Box::new(ChannelBus(tx))).unwrap();

        client.disconnect().unwrap();
        client.disconnect().unwrap();

        assert_eq!(
            connection_reports(rx.try_iter()),
            vec![(Connection::Orders, ConnectionState::Down)]
        );
    }

    #[test]
    fn disconnecting_the_data_client_reports_its_feed_down_once_the_beat_stops() {
        let mut client = SandboxDataClient::new(listed(&["AAA"]));
        let (tx, rx) = mpsc::channel();
        client.connect(Box::new(ChannelBus(tx))).unwrap();

        client.disconnect().unwrap();

        let deadline = Instant::now() + Duration::from_secs(5);
        let reports = connection_reports(std::iter::from_fn(|| {
            rx.recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .ok()
        }));
        assert_eq!(
            reports,
            vec![(Connection::MarketData, ConnectionState::Down)]
        );
    }
}
