//! Typed in-process pub/sub between adapters, engines, and the UI shell.
//! See /docs/architecture.md § "One message bus, three consumers".

use std::sync::{mpsc, Arc, Mutex, MutexGuard, PoisonError};

use model::{
    AccountState, ConnectionStatus, Depth, ExecutionReport, InstrumentInfo, PriceBand, Quote,
    SessionStatus, Trade,
};

/// Everything an adapter can tell core. There is deliberately no variant
/// carrying an [`model::Order`]: adapters report what the venue said, and the
/// execution engine alone decides what that does to order state.
///
/// New variants may be added in a minor release, so code that matches on
/// this needs a wildcard arm.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum Event {
    Quote(Quote),
    Trade(Trade),
    Execution(ExecutionReport),
    AccountUpdate(AccountState),
    /// How an instrument trades: lot size, tick sizes, listing board.
    Instrument(InstrumentInfo),
    /// Today's reference price and, where the market has them, limits.
    PriceBand(PriceBand),
    /// Top levels of the book, for venues that publish depth.
    Depth(Depth),
    /// One of the adapter's connections changed: lost, degraded, back.
    Connection(ConnectionStatus),
    /// A market moved to another phase of its trading day, which changes the
    /// order types it takes.
    Session(SessionStatus),
}

/// Handle an adapter uses to push events onto the bus.
/// `connect()` in both adapter traits receives one of these.
pub trait BusHandle: Send {
    fn publish(&self, event: Event);
}

#[derive(Default)]
struct Subscribers {
    senders: Mutex<Vec<mpsc::Sender<Event>>>,
}

impl Subscribers {
    /// A publisher that panicked mid-publish leaves the list intact (a `Vec`
    /// of senders has no half-written state), so poisoning is safe to ignore.
    fn lock(&self) -> MutexGuard<'_, Vec<mpsc::Sender<Event>>> {
        self.senders.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Fan-out bus. Every subscriber gets its own unbounded channel, so a slow
/// consumer can never block a publisher or another consumer. The cost is
/// memory: a consumer that stops draining grows without bound.
#[derive(Clone, Default)]
pub struct Bus {
    subscribers: Arc<Subscribers>,
}

impl Bus {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn subscribe(&self) -> mpsc::Receiver<Event> {
        let (tx, rx) = mpsc::channel();
        self.subscribers.lock().push(tx);
        rx
    }

    pub fn publish(&self, event: Event) {
        self.subscribers
            .lock()
            .retain(|tx| tx.send(event.clone()).is_ok());
    }

    pub fn handle(&self) -> Box<dyn BusHandle> {
        Box::new(Handle { bus: self.clone() })
    }
}

struct Handle {
    bus: Bus,
}

impl BusHandle for Handle {
    fn publish(&self, event: Event) {
        self.bus.publish(event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use model::{Decimal, InstrumentId, Side, UnixNanos};

    fn trade() -> Trade {
        Trade {
            instrument: InstrumentId::new("T", "X"),
            price: Decimal::ONE,
            quantity: Decimal::ONE,
            side: Some(Side::Buy),
            ts_event: UnixNanos(0),
        }
    }

    #[test]
    fn every_subscriber_receives_each_event() {
        let bus = Bus::new();
        let a = bus.subscribe();
        let b = bus.subscribe();

        bus.publish(Event::Trade(trade()));

        assert!(matches!(a.try_recv(), Ok(Event::Trade(_))));
        assert!(matches!(b.try_recv(), Ok(Event::Trade(_))));
    }

    #[test]
    fn dropped_subscriber_is_pruned_without_affecting_others() {
        let bus = Bus::new();
        let gone = bus.subscribe();
        let alive = bus.subscribe();
        drop(gone);

        bus.publish(Event::Trade(trade()));

        assert!(matches!(alive.try_recv(), Ok(Event::Trade(_))));
        assert_eq!(bus.subscribers.lock().len(), 1);
    }

    #[test]
    fn handle_publishes_onto_the_same_bus() {
        let bus = Bus::new();
        let rx = bus.subscribe();
        let handle = bus.handle();

        handle.publish(Event::Trade(trade()));

        assert!(matches!(rx.try_recv(), Ok(Event::Trade(_))));
    }

    #[test]
    fn publishing_still_works_after_a_subscriber_thread_panicked_holding_the_lock() {
        let bus = Bus::new();
        let rx = bus.subscribe();
        let poisoner = bus.clone();
        let _ = std::thread::spawn(move || {
            let _guard = poisoner.subscribers.senders.lock();
            panic!("poison the subscriber list");
        })
        .join();

        bus.publish(Event::Trade(trade()));

        assert!(matches!(rx.try_recv(), Ok(Event::Trade(_))));
    }
}
