//! Domain types. No I/O, no async — pure data + invariants.
//! See /docs/architecture.md for how this fits the rest of the workspace.

/// A venue-qualified instrument identity, e.g. "BINANCE:BTCUSDT".
/// Deliberately opaque to core: adapters decide what the symbol part means.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct InstrumentId {
    pub venue: String,
    pub symbol: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct OrderId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Buy,
    Sell,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderStatus {
    New,
    Submitted,
    PartiallyFilled,
    Filled,
    Cancelled,
    Rejected,
}

#[derive(Debug, Clone)]
pub struct NewOrder {
    pub instrument: InstrumentId,
    pub side: Side,
    pub quantity: f64,
    pub limit_price: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct Order {
    pub id: OrderId,
    pub instrument: InstrumentId,
    pub side: Side,
    pub quantity: f64,
    pub filled_quantity: f64,
    pub limit_price: Option<f64>,
    pub status: OrderStatus,
}

#[derive(Debug, Clone)]
pub struct Quote {
    pub instrument: InstrumentId,
    pub bid: f64,
    pub ask: f64,
    pub bid_size: f64,
    pub ask_size: f64,
}

#[derive(Debug, Clone)]
pub struct Trade {
    pub instrument: InstrumentId,
    pub price: f64,
    pub quantity: f64,
    pub side: Side,
}

#[derive(Debug, Clone, Default)]
pub struct Position {
    pub instrument: Option<InstrumentId>,
    pub quantity: f64,
    pub avg_price: f64,
}

#[derive(Debug, Clone, Default)]
pub struct AccountState {
    pub cash: f64,
    pub positions: Vec<Position>,
}
