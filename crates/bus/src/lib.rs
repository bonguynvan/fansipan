//! Typed pub/sub between adapters, engines, and the UI shell.
//! Stub: Phase 0 fills this in with a real channel-backed implementation.
//! See /docs/architecture.md § "One message bus, three consumers".

use model::{AccountState, Order, Quote, Trade};

#[derive(Debug, Clone)]
pub enum Event {
    Quote(Quote),
    Trade(Trade),
    OrderUpdate(Order),
    AccountUpdate(AccountState),
}

/// Handle an adapter uses to push events onto the bus.
/// `connect()` in both adapter traits receives one of these.
pub trait BusHandle: Send {
    fn publish(&self, event: Event);
}
