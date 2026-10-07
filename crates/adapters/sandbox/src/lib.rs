//! Simulated venue for demos, CI, and development. Not a real venue: no
//! network, no protocol. A random walk moves prices; market orders sweep,
//! limit orders rest and fill when the walk reaches them, at most
//! [`TOP_OF_BOOK_SIZE`] per order per price move.
//!
//! The two clients are thin: [`SimVenue`] does the matching and the clients
//! only move its events onto the bus, which is the shape a real adapter has
//! too (decode, translate, publish).

mod venue;

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use adapter_api::{AdapterError, DataClient, ExecutionClient};
use bus::{BusHandle, Event};
use model::{AccountState, CancelOrder, InstrumentId, SubmitOrder, UnixNanos};

pub use venue::{SimVenue, MIN_MID, SPREAD, TICK, TOP_OF_BOOK_SIZE};

const TICK_INTERVAL: Duration = Duration::from_millis(500);
const WALK_SEED: u64 = 0x9E37_79B9_7F4A_7C15;
/// Quotes of made-up history sent when an instrument is first subscribed:
/// twenty minutes at the beat's pace, so a chart opens with something to
/// show instead of filling up one bar at a time.
const HISTORY_QUOTES: usize = 2_400;

type Subscriptions = Arc<Mutex<HashSet<InstrumentId>>>;

pub struct SandboxDataClient {
    venue: SimVenue,
    subscribed: Subscriptions,
    running: Arc<AtomicBool>,
}

impl SandboxDataClient {
    pub fn new(venue: SimVenue) -> Self {
        Self {
            venue,
            subscribed: Arc::default(),
            running: Arc::default(),
        }
    }

    fn subscriptions(&self) -> std::sync::MutexGuard<'_, HashSet<InstrumentId>> {
        self.subscribed
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
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
        let subscribed = self.subscribed.clone();
        thread::Builder::new()
            .name("sandbox-market".into())
            .spawn(move || {
                let mut seed = WALK_SEED;
                let mut backfilled = HashSet::new();
                while running.load(Ordering::SeqCst) {
                    let watched = subscribed
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .clone();
                    let beat = Beat {
                        venue: &venue,
                        subscribed: &watched,
                        bus: bus.as_ref(),
                    };
                    beat.run(&mut backfilled, &mut seed, now());
                    thread::sleep(TICK_INTERVAL);
                }
            })
            .map(drop)
            .map_err(|e| {
                self.running.store(false, Ordering::SeqCst);
                AdapterError::fatal(format!("sandbox: cannot start market thread: {e}"))
            })
    }

    fn subscribe_quotes(&mut self, instrument: &InstrumentId) -> Result<(), AdapterError> {
        if !self.venue.is_listed(instrument) {
            return Err(AdapterError::fatal(format!(
                "sandbox: {instrument} is not listed"
            )));
        }
        self.subscriptions().insert(instrument.clone());
        Ok(())
    }

    fn unsubscribe_quotes(&mut self, instrument: &InstrumentId) -> Result<(), AdapterError> {
        self.subscriptions().remove(instrument);
        Ok(())
    }

    /// The sandbox prints no tape; trade subscriptions are accepted and
    /// produce nothing.
    fn subscribe_trades(&mut self, _instrument: &InstrumentId) -> Result<(), AdapterError> {
        Ok(())
    }

    fn unsubscribe_trades(&mut self, _instrument: &InstrumentId) -> Result<(), AdapterError> {
        Ok(())
    }

    fn disconnect(&mut self) -> Result<(), AdapterError> {
        self.running.store(false, Ordering::SeqCst);
        Ok(())
    }
}

/// What one beat of the simulated market reads.
struct Beat<'a> {
    venue: &'a SimVenue,
    subscribed: &'a HashSet<InstrumentId>,
    bus: &'a dyn BusHandle,
}

impl Beat<'_> {
    /// Newly subscribed instruments get their made-up history first. Then
    /// every listed instrument takes a random step; quotes go out for the
    /// subscribed ones, fills for all. `backfilled` forgets instruments that
    /// were unsubscribed, so subscribing again brings a fresh history.
    fn run(&self, backfilled: &mut HashSet<InstrumentId>, seed: &mut u64, now: UnixNanos) {
        backfilled.retain(|instrument| self.subscribed.contains(instrument));
        for instrument in self.subscribed {
            if backfilled.insert(instrument.clone()) {
                self.venue
                    .synthetic_history(instrument, now, HISTORY_QUOTES, TICK_INTERVAL, seed)
                    .into_iter()
                    .for_each(|quote| self.bus.publish(Event::Quote(quote)));
            }
        }

        for instrument in self.venue.instruments() {
            let step = venue::random_step(seed);
            let publish = &mut |event| self.bus.publish(event);
            let Some(quote) = self.venue.step(&instrument, step, now, publish) else {
                continue;
            };
            if self.subscribed.contains(&instrument) {
                self.bus.publish(Event::Quote(quote));
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
        self.bus = None;
        Ok(())
    }
}

fn now() -> UnixNanos {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_nanos());
    UnixNanos(u64::try_from(nanos).unwrap_or(u64::MAX))
}

fn not_connected() -> AdapterError {
    AdapterError::fatal("sandbox: not connected")
}

#[cfg(test)]
mod tests {
    use super::*;
    use model::{ClientOrderId, Decimal, Side};
    use std::sync::mpsc;

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

    #[test]
    fn a_beat_publishes_quotes_only_for_subscribed_instruments() {
        let venue = listed(&["AAA", "BBB"]);
        let (tx, rx) = mpsc::channel();
        let bus = ChannelBus(tx);
        let watched = HashSet::from([InstrumentId::new("sim", "AAA")]);
        let beat = Beat {
            venue: &venue,
            subscribed: &watched,
            bus: &bus,
        };
        let mut backfilled = watched.clone();

        beat.run(&mut backfilled, &mut 7, UnixNanos(1));

        assert_eq!(quoted_symbols(&rx), vec!["AAA".to_string()]);
    }

    #[test]
    fn a_new_subscription_is_backfilled_once() {
        let venue = listed(&["AAA"]);
        let (tx, rx) = mpsc::channel();
        let bus = ChannelBus(tx);
        let watched = HashSet::from([InstrumentId::new("sim", "AAA")]);
        let beat = Beat {
            venue: &venue,
            subscribed: &watched,
            bus: &bus,
        };
        let mut backfilled = HashSet::new();
        let now = UnixNanos(10_000_000_000_000);

        beat.run(&mut backfilled, &mut 7, now);
        let first = quoted_symbols(&rx).len();
        beat.run(&mut backfilled, &mut 7, now);
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
        let mut backfilled = HashSet::from([InstrumentId::new("sim", "AAA")]);
        let idle = Beat {
            venue: &venue,
            subscribed: &nobody,
            bus: &bus,
        };

        idle.run(&mut backfilled, &mut 7, UnixNanos(10_000_000_000_000));

        assert!(backfilled.is_empty());
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

        let result = client.submit_order(SubmitOrder {
            order_id: ClientOrderId(1),
            instrument: InstrumentId::new("sim", "AAA"),
            side: Side::Buy,
            quantity: Decimal::ONE,
            limit_price: None,
        });

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
}
