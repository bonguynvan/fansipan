use rust_decimal::Decimal;

use crate::{ClientOrderId, FillId, InstrumentId, OrderType, UnixNanos, VenueOrderId};

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
    /// An amendment was sent; the venue has not answered yet.
    PendingAmend,
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
            Self::Submitted
                | Self::Accepted
                | Self::PartiallyFilled
                | Self::PendingCancel
                | Self::PendingAmend
        )
    }
}

/// What the user (or a strategy) asked for. Carries no id: the execution
/// engine assigns one when it creates the order.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct NewOrder {
    pub instrument: InstrumentId,
    pub side: Side,
    pub quantity: Decimal,
    pub order_type: OrderType,
    /// Set for a `Limit` order, `None` for every other kind.
    pub limit_price: Option<Decimal>,
}

impl NewOrder {
    /// Any order type. `limit_price` must be set exactly when the type is a
    /// limit order; risk denies an order where it is not.
    pub fn new(
        instrument: InstrumentId,
        side: Side,
        quantity: Decimal,
        order_type: OrderType,
        limit_price: Option<Decimal>,
    ) -> Self {
        Self {
            instrument,
            side,
            quantity,
            order_type,
            limit_price,
        }
    }

    /// A limit order for the day.
    pub fn limit(instrument: InstrumentId, side: Side, quantity: Decimal, price: Decimal) -> Self {
        Self::new(instrument, side, quantity, OrderType::LIMIT, Some(price))
    }

    /// A market order for the day.
    pub fn market(instrument: InstrumentId, side: Side, quantity: Decimal) -> Self {
        Self::new(instrument, side, quantity, OrderType::MARKET, None)
    }
}

/// The order an adapter is asked to place: a risk-cleared [`NewOrder`] plus
/// the id every execution report for it must carry.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct SubmitOrder {
    pub order_id: ClientOrderId,
    pub instrument: InstrumentId,
    pub side: Side,
    pub quantity: Decimal,
    pub order_type: OrderType,
    /// Set for a `Limit` order, `None` for every other kind.
    pub limit_price: Option<Decimal>,
}

impl SubmitOrder {
    pub fn new(order_id: ClientOrderId, order: NewOrder) -> Self {
        Self {
            order_id,
            instrument: order.instrument,
            side: order.side,
            quantity: order.quantity,
            order_type: order.order_type,
            limit_price: order.limit_price,
        }
    }
}

/// Everything most venues need to cancel: our id, theirs if the venue
/// already accepted the order, and the instrument (some venues key cancels
/// by symbol plus venue id).
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct CancelOrder {
    pub order_id: ClientOrderId,
    pub venue_order_id: Option<VenueOrderId>,
    pub instrument: InstrumentId,
}

impl CancelOrder {
    pub fn new(
        order_id: ClientOrderId,
        venue_order_id: Option<VenueOrderId>,
        instrument: InstrumentId,
    ) -> Self {
        Self {
            order_id,
            venue_order_id,
            instrument,
        }
    }
}

/// New terms for a working order: its new total quantity (what has filled
/// stays filled) and its new limit price.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct AmendOrder {
    pub order_id: ClientOrderId,
    pub venue_order_id: Option<VenueOrderId>,
    pub instrument: InstrumentId,
    pub quantity: Decimal,
    pub limit_price: Option<Decimal>,
}

impl AmendOrder {
    pub fn new(
        order_id: ClientOrderId,
        venue_order_id: Option<VenueOrderId>,
        instrument: InstrumentId,
        quantity: Decimal,
        limit_price: Option<Decimal>,
    ) -> Self {
        Self {
            order_id,
            venue_order_id,
            instrument,
            quantity,
            limit_price,
        }
    }
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
    pub order_type: OrderType,
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
    /// The venue changed the order to these terms: its total quantity and
    /// its limit price.
    Amended {
        quantity: Decimal,
        limit_price: Option<Decimal>,
    },
    /// The venue refused to change the order; it stands as it was.
    AmendRejected {
        reason: String,
    },
    /// The venue's view of the order now, read back after a reconnect:
    /// the execution engine catches up from it on missed fills and ends.
    Snapshot(VenueOrder),
}

/// Where an order stands on the venue.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum VenueOrderState {
    /// Still open: resting, or waiting for an auction.
    Working,
    Filled,
    /// Ended without filling in full: cancelled, expired, or killed.
    Cancelled,
    Rejected {
        reason: String,
    },
}

/// One order as the venue holds it now: its state, and its cumulative fills.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct VenueOrder {
    pub venue_order_id: Option<VenueOrderId>,
    pub state: VenueOrderState,
    /// Everything filled so far, not only what the stream missed.
    pub filled_quantity: Decimal,
    /// Over every fill so far; `None` until the first.
    pub avg_fill_price: Option<Decimal>,
    /// The order's total quantity on the venue now, if the adapter read it.
    /// It settles an amendment whose answer the stream missed.
    pub quantity: Option<Decimal>,
    /// The order's limit price on the venue now, if the adapter read it.
    pub limit_price: Option<Decimal>,
}

impl VenueOrder {
    pub fn new(
        state: VenueOrderState,
        filled_quantity: Decimal,
        avg_fill_price: Option<Decimal>,
    ) -> Self {
        Self {
            venue_order_id: None,
            state,
            filled_quantity,
            avg_fill_price,
            quantity: None,
            limit_price: None,
        }
    }

    /// The order's terms on the venue now: its total quantity and limit.
    pub fn with_terms(self, quantity: Decimal, limit_price: Option<Decimal>) -> Self {
        Self {
            quantity: Some(quantity),
            limit_price,
            ..self
        }
    }

    pub fn with_venue_order_id(self, venue_order_id: VenueOrderId) -> Self {
        Self {
            venue_order_id: Some(venue_order_id),
            ..self
        }
    }
}
