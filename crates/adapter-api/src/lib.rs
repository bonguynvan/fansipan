//! The one crate an external adapter repo depends on.
//! Full rationale + required/forbidden behavior: /docs/adapter-contract.md
//! Breaking changes here are a last resort — see that doc's "Versioning" section.

use bus::BusHandle;
use model::{AccountState, InstrumentId, NewOrder, OrderId};

#[derive(Debug, Clone)]
pub struct AdapterError {
    pub message: String,
    pub recoverable: bool,
}

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

    fn subscribe_quotes(&mut self, instrument: InstrumentId) -> Result<(), AdapterError>;
    fn unsubscribe_quotes(&mut self, instrument: InstrumentId) -> Result<(), AdapterError>;

    fn subscribe_trades(&mut self, instrument: InstrumentId) -> Result<(), AdapterError>;
    fn unsubscribe_trades(&mut self, instrument: InstrumentId) -> Result<(), AdapterError>;

    fn disconnect(&mut self) -> Result<(), AdapterError>;
}

/// Submits and manages orders for a venue. One instance per venue account.
/// Required/forbidden behavior: /docs/adapter-contract.md
pub trait ExecutionClient: Send {
    fn connect(&mut self, bus: Box<dyn BusHandle>) -> Result<(), AdapterError>;

    fn submit_order(&mut self, order: NewOrder) -> Result<(), AdapterError>;
    fn cancel_order(&mut self, order_id: OrderId) -> Result<(), AdapterError>;
    fn account_state(&mut self) -> Result<AccountState, AdapterError>;

    fn disconnect(&mut self) -> Result<(), AdapterError>;
}
