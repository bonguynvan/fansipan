use rust_decimal::Decimal;

use crate::{ClientOrderId, FillId, InstrumentId, UnixNanos, VenueOrderId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Side {
    Buy,
    Sell,
}

/// Where an order is in its life. Transitions are decided by the execution
/// engine alone; see /docs/order-lifecycle.md for the full table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum OrderStatus {
    /// Handed to the adapter; the venue has not answered yet.
    Submitted,
    /// The venue confirmed it holds the order.
    Accepted,
    PartiallyFilled,
    Filled,
    /// A cancel was sent; the venue has not confirmed it yet.
    PendingCancel,
    Cancelled,
    /// The venue (or the adapter) refused the order.
    Rejected,
    /// Pre-trade risk refused the order. It never reached an adapter.
    Denied,
}

impl OrderStatus {
    /// Still able to trade, or waiting to learn whether it can.
    pub fn is_open(self) -> bool {
        matches!(
            self,
            Self::Submitted | Self::Accepted | Self::PartiallyFilled | Self::PendingCancel
        )
    }
}

/// What the user (or a strategy) asked for. Carries no id: the execution
/// engine assigns one when it creates the order.
#[derive(Debug, Clone, PartialEq)]
pub struct NewOrder {
    pub instrument: InstrumentId,
    pub side: Side,
    pub quantity: Decimal,
    /// `None` is a market order.
    pub limit_price: Option<Decimal>,
}

/// The order an adapter is asked to place: a risk-cleared [`NewOrder`] plus
/// the id every execution report for it must carry.
#[derive(Debug, Clone, PartialEq)]
pub struct SubmitOrder {
    pub order_id: ClientOrderId,
    pub instrument: InstrumentId,
    pub side: Side,
    pub quantity: Decimal,
    pub limit_price: Option<Decimal>,
}

/// Everything most venues need to cancel: our id, theirs if the venue
/// already accepted the order, and the instrument (some venues key cancels
/// by symbol plus venue id).
#[derive(Debug, Clone, PartialEq)]
pub struct CancelOrder {
    pub order_id: ClientOrderId,
    pub venue_order_id: Option<VenueOrderId>,
    pub instrument: InstrumentId,
}

/// The execution engine's view of one order. Adapters never build or mutate
/// this; they emit [`ExecutionReport`]s and the engine derives it.
#[derive(Debug, Clone, PartialEq)]
pub struct Order {
    pub id: ClientOrderId,
    pub venue_order_id: Option<VenueOrderId>,
    pub instrument: InstrumentId,
    pub side: Side,
    pub quantity: Decimal,
    pub limit_price: Option<Decimal>,
    pub status: OrderStatus,
    pub filled_quantity: Decimal,
    /// Volume-weighted over every applied fill; `None` until the first.
    pub avg_fill_price: Option<Decimal>,
    /// Why the order was denied or rejected, or why the last cancel was refused.
    pub reason: Option<String>,
}

impl Order {
    pub fn remaining(&self) -> Decimal {
        self.quantity - self.filled_quantity
    }
}

/// What a venue said about one order. The only way an adapter influences
/// order state.
#[derive(Debug, Clone, PartialEq)]
pub struct ExecutionReport {
    pub order_id: ClientOrderId,
    pub event: OrderEvent,
    pub ts_event: UnixNanos,
}

/// New variants may be added in a minor release, so code that matches on this
/// needs a wildcard arm.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum OrderEvent {
    Accepted {
        venue_order_id: VenueOrderId,
    },
    Rejected {
        reason: String,
    },
    /// One execution: `quantity` and `price` are this fill's, not cumulative.
    Filled {
        fill_id: FillId,
        quantity: Decimal,
        price: Decimal,
    },
    Cancelled,
    /// The venue refused a cancel, typically because the order already
    /// filled or was never open there.
    CancelRejected {
        reason: String,
    },
}
