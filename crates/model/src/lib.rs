//! Domain types. No I/O, no async — pure data + invariants.
//! See /docs/architecture.md for how this fits the rest of the workspace.
//!
//! Prices, quantities, and cash are [`Decimal`]: binary floats cannot represent
//! most decimal prices exactly, and order-book and P&L arithmetic must not
//! drift. See /docs/adr/0001-decimal-money.md.

mod account;
mod connection;
mod ids;
mod market;
mod order;
mod order_type;
mod rules;
mod session;

pub use rust_decimal::Decimal;

pub use account::{AccountState, Position};
pub use connection::{Connection, ConnectionState, ConnectionStatus};
pub use ids::{ClientOrderId, FillId, InstrumentId, UnixNanos, VenueOrderId};
pub use market::{Quote, Trade};
pub use order::{
    AmendOrder, CancelOrder, ExecutionReport, NewOrder, Order, OrderEvent, OrderStatus, Side,
    SubmitOrder,
};
pub use order_type::{NamedOrderType, OrderKind, OrderType, TimeInForce};
pub use rules::{
    BandError, BookLevel, Depth, InstrumentInfo, PriceBand, TickTable, TickTableError,
};
pub use session::{SessionPhase, SessionStatus};
