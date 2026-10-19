//! What the engine records so a restart can bring the session back, and
//! what it is rebuilt from (ADR 0023). A record is the whole current state
//! of one thing, so rebuilding is a fold with the last record winning and
//! sends nothing to the venue.

use std::fmt;

use model::{Decimal, FillId, Order};

use crate::emulator::Conditional;
use crate::risk::TradingState;

/// An order as the engine holds it: the order, the venue fills already
/// applied to it (so the same fill reported again after a restart is not
/// counted twice), and the total an amendment in flight would leave.
#[derive(Debug, Clone, PartialEq)]
pub struct OrderRecord {
    pub order: Order,
    pub applied_fills: Vec<FillId>,
    pub amending_to: Option<Decimal>,
    /// Placed elsewhere and adopted (ADR 0024).
    pub external: bool,
}

/// The session as recorded: every order and conditional order, the
/// trader's switch, and the order ids reserved before a crash could use
/// them, which new ids must start past.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Restored {
    pub orders: Vec<OrderRecord>,
    pub conditionals: Vec<Conditional>,
    pub trading: Option<TradingState>,
    /// The first order id no earlier session can have sent.
    pub next_order_id: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RestoreError {
    /// The engine already holds orders or conditional orders of its own.
    NotEmpty,
}

impl fmt::Display for RestoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotEmpty => {
                f.write_str("a session can be restored only into an engine that holds none")
            }
        }
    }
}

impl std::error::Error for RestoreError {}
