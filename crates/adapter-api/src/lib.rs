//! The one crate an external adapter repo depends on.
//! Full rationale + required/forbidden behavior: /docs/adapter-contract.md
//! Breaking changes here are a last resort — see that doc's "Versioning" section.
//!
//! `model` and `bus` are re-exported so an adapter pins one version of the
//! contract instead of three crates that must agree with each other.
//!
//! # A minimal adapter
//!
//! A venue that quotes one price and takes every order. Each call publishes
//! what the venue would answer and returns at once. A real adapter hands
//! each call to its own connection thread and publishes the venue's answers
//! from there, since a call must never wait on the network. It also:
//!
//! - publishes each instrument's rules, today's price band, the session,
//!   and its connections' state (`Instrument`, `PriceBand`, `Session`,
//!   `Connection`), without which risk has no tick, band, or session to
//!   check against;
//! - answers a cancel for an order that is not open with `CancelRejected`,
//!   where this one confirms every cancel;
//! - reads the account from the venue, and publishes `AccountUpdate` when
//!   it changes.
//!
//! ```
//! use fansipan_adapter_api::bus::{Bus, BusHandle, Event};
//! use fansipan_adapter_api::model::{
//!     AccountState, CancelOrder, ClientOrderId, Decimal, ExecutionReport, InstrumentId,
//!     NewOrder, OrderEvent, Quote, Side, SubmitOrder, UnixNanos, VenueOrderId,
//! };
//! use fansipan_adapter_api::{AdapterError, DataClient, ExecutionClient};
//! use std::time::{SystemTime, UNIX_EPOCH};
//!
//! /// When the venue sends no time of its own, the time it was received.
//! fn now() -> UnixNanos {
//!     let since = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
//!     UnixNanos(u64::try_from(since.as_nanos()).unwrap_or(u64::MAX))
//! }
//!
//! /// Where a client publishes, once connected.
//! #[derive(Default)]
//! struct Wire(Option<Box<dyn BusHandle>>);
//!
//! impl Wire {
//!     fn publish(&self, event: Event) -> Result<(), AdapterError> {
//!         let bus = self.0.as_ref().ok_or_else(|| AdapterError::recoverable("not connected"))?;
//!         bus.publish(event);
//!         Ok(())
//!     }
//! }
//!
//! /// Quotes 100.00 / 100.10 for whatever is subscribed.
//! #[derive(Default)]
//! struct Prices(Wire);
//!
//! impl DataClient for Prices {
//!     fn connect(&mut self, bus: Box<dyn BusHandle>) -> Result<(), AdapterError> {
//!         self.0 = Wire(Some(bus));
//!         Ok(())
//!     }
//!     fn subscribe_quotes(&mut self, instrument: &InstrumentId) -> Result<(), AdapterError> {
//!         self.0.publish(Event::Quote(Quote {
//!             instrument: instrument.clone(),
//!             bid: Decimal::new(10_000, 2),
//!             ask: Decimal::new(10_010, 2),
//!             bid_size: Decimal::from(500),
//!             ask_size: Decimal::from(300),
//!             ts_event: now(),
//!         }))
//!     }
//!     fn unsubscribe_quotes(&mut self, _: &InstrumentId) -> Result<(), AdapterError> {
//!         Ok(())
//!     }
//!     fn subscribe_trades(&mut self, _: &InstrumentId) -> Result<(), AdapterError> {
//!         Ok(())
//!     }
//!     fn unsubscribe_trades(&mut self, _: &InstrumentId) -> Result<(), AdapterError> {
//!         Ok(())
//!     }
//!     fn disconnect(&mut self) -> Result<(), AdapterError> {
//!         self.0 = Wire(None);
//!         Ok(())
//!     }
//! }
//!
//! /// Accepts every order and every cancel.
//! #[derive(Default)]
//! struct Orders {
//!     wire: Wire,
//!     next_id: u64,
//! }
//!
//! impl Orders {
//!     fn report(&self, order_id: ClientOrderId, event: OrderEvent) -> Result<(), AdapterError> {
//!         let report = ExecutionReport { order_id, event, ts_event: now() };
//!         self.wire.publish(Event::Execution(report))
//!     }
//! }
//!
//! impl ExecutionClient for Orders {
//!     fn connect(&mut self, bus: Box<dyn BusHandle>) -> Result<(), AdapterError> {
//!         self.wire = Wire(Some(bus));
//!         Ok(())
//!     }
//!     fn submit_order(&mut self, order: SubmitOrder) -> Result<(), AdapterError> {
//!         self.next_id += 1;
//!         let venue_order_id = VenueOrderId(format!("V{}", self.next_id));
//!         self.report(order.order_id, OrderEvent::Accepted { venue_order_id })
//!     }
//!     fn cancel_order(&mut self, cancel: CancelOrder) -> Result<(), AdapterError> {
//!         self.report(cancel.order_id, OrderEvent::Cancelled)
//!     }
//!     fn account_state(&mut self) -> Result<AccountState, AdapterError> {
//!         Ok(AccountState::new(Decimal::from(1_000_000), Vec::new()))
//!     }
//!     fn disconnect(&mut self) -> Result<(), AdapterError> {
//!         self.wire = Wire(None);
//!         Ok(())
//!     }
//! }
//!
//! // Core does this part: one bus, both clients connected to it, and risk
//! // run on every order before it reaches `submit_order`.
//! let bus = Bus::new();
//! let events = bus.subscribe();
//! let (mut prices, mut orders) = (Prices::default(), Orders::default());
//! prices.connect(bus.handle())?;
//! orders.connect(bus.handle())?;
//!
//! let abc = InstrumentId::new("sim", "ABC");
//! prices.subscribe_quotes(&abc)?;
//! let order = NewOrder::limit(abc, Side::Buy, Decimal::from(100), Decimal::new(10_000, 2));
//! orders.submit_order(SubmitOrder::new(ClientOrderId(1), order))?;
//!
//! let first = events.try_recv();
//! assert!(matches!(first, Ok(Event::Quote(quote)) if quote.ask == Decimal::new(10_010, 2)));
//! match events.try_recv() {
//!     Ok(Event::Execution(report)) => {
//!         assert_eq!(report.order_id, ClientOrderId(1));
//!         let venue_order_id = VenueOrderId("V1".into());
//!         assert_eq!(report.event, OrderEvent::Accepted { venue_order_id });
//!     }
//!     other => panic!("expected the order's acceptance, got {other:?}"),
//! }
//! # Ok::<(), AdapterError>(())
//! ```

use std::fmt;

pub use bus;
pub use model;

use bus::BusHandle;
use model::{AccountState, AmendOrder, CancelOrder, InstrumentId, RecoverOrder, SubmitOrder};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdapterError {
    pub message: String,
    /// Whether retrying the same call later could succeed (a dropped
    /// connection) or not (bad credentials, an unknown instrument).
    pub recoverable: bool,
}

impl AdapterError {
    pub fn recoverable(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            recoverable: true,
        }
    }

    pub fn fatal(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            recoverable: false,
        }
    }
}

impl fmt::Display for AdapterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for AdapterError {}

pub struct AdapterConfig {
    pub venue: String,
    /// Opaque to core: each adapter defines its own settings shape and
    /// parses it out of this, e.g. via a TOML/JSON blob under a key it owns.
    pub settings: serde_json::Value,
}

/// Streams market data for a venue. One instance per venue connection.
/// Required/forbidden behavior: /docs/adapter-contract.md
pub trait DataClient: Send {
    fn connect(&mut self, bus: Box<dyn BusHandle>) -> Result<(), AdapterError>;

    fn subscribe_quotes(&mut self, instrument: &InstrumentId) -> Result<(), AdapterError>;
    fn unsubscribe_quotes(&mut self, instrument: &InstrumentId) -> Result<(), AdapterError>;

    fn subscribe_trades(&mut self, instrument: &InstrumentId) -> Result<(), AdapterError>;
    fn unsubscribe_trades(&mut self, instrument: &InstrumentId) -> Result<(), AdapterError>;

    fn disconnect(&mut self) -> Result<(), AdapterError>;
}

/// Submits and manages orders for a venue. One instance per venue account.
/// Required/forbidden behavior: /docs/adapter-contract.md
pub trait ExecutionClient: Send {
    fn connect(&mut self, bus: Box<dyn BusHandle>) -> Result<(), AdapterError>;

    /// Send a risk-cleared order to the venue. Returning `Ok` means "handed
    /// off", not "accepted": the venue's answer arrives later as an
    /// `Event::Execution` keyed by `order.order_id`.
    fn submit_order(&mut self, order: SubmitOrder) -> Result<(), AdapterError>;

    /// Ask the venue to cancel. The outcome arrives later as
    /// `OrderEvent::Cancelled` or `OrderEvent::CancelRejected`.
    fn cancel_order(&mut self, cancel: CancelOrder) -> Result<(), AdapterError>;

    fn account_state(&mut self) -> Result<AccountState, AdapterError>;

    fn disconnect(&mut self) -> Result<(), AdapterError>;

    /// This client's amendments, if its venue can change a working order.
    /// The default says it cannot, which is the truth for an adapter that
    /// does not implement [`OrderAmender`]: core then refuses amendments
    /// before they reach it.
    fn amender(&mut self) -> Option<&mut dyn OrderAmender> {
        None
    }

    /// The account this client trades, stable across restarts, once
    /// connected. Core keeps a journal per account and restores it at
    /// start (ADR 0023); it stores only a hash of this. The default names
    /// none, and core then keeps no journal: right for a venue whose
    /// account does not outlive the process, such as a simulated one.
    fn account_id(&self) -> Option<String> {
        None
    }

    /// This client's answers about orders the terminal knew before it
    /// restarted, if its venue can say. The default says it cannot: core
    /// then shows those orders as not yet confirmed by the venue.
    fn recovery(&mut self) -> Option<&mut dyn OrderRecovery> {
        None
    }
}

/// Reports the venue's view of orders the terminal held before it
/// restarted. An adapter implements it on its execution client and returns
/// itself from [`ExecutionClient::recovery`].
pub trait OrderRecovery: Send {
    /// Ask the venue about `orders`. Returning `Ok` means "asked": for each
    /// order the venue knows, the answer arrives later as an
    /// `Event::Execution` carrying `OrderEvent::Snapshot`, exactly as after
    /// a reconnect (ADR 0012). An order the venue does not know is left
    /// unanswered, and stays unconfirmed.
    fn recover_orders(&mut self, orders: Vec<RecoverOrder>) -> Result<(), AdapterError>;
}

/// Changes a working order's quantity or price, for a venue that can. An
/// adapter implements it on its execution client and returns itself from
/// [`ExecutionClient::amender`].
pub trait OrderAmender: Send {
    /// Ask the venue to amend. Returning `Ok` means "handed off": the answer
    /// arrives later as `OrderEvent::Amended` or `OrderEvent::AmendRejected`.
    fn amend_order(&mut self, amend: AmendOrder) -> Result<(), AdapterError>;
}
