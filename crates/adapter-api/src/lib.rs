//! The one crate an external adapter repo depends on.
//! Full rationale + required/forbidden behavior: /docs/adapter-contract.md
//! Breaking changes here are a last resort — see that doc's "Versioning" section.
//!
//! `model` and `bus` are re-exported so an adapter pins one version of the
//! contract instead of three crates that must agree with each other.

use std::fmt;

pub use bus;
pub use model;

use bus::BusHandle;
use model::{AccountState, CancelOrder, InstrumentId, SubmitOrder};

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
}
