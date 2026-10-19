//! The engine's one hold on the adapter's execution client. An order or an
//! amendment reaches the client only as a [`Cleared`] value, which only
//! [`crate::risk`] makes, so the compiler, not a review, keeps an order risk
//! has not cleared from reaching an adapter.

use adapter_api::{AdapterError, ExecutionClient};
use bus::BusHandle;
use model::{AmendOrder, CancelOrder, RecoverOrder, SubmitOrder};

use crate::risk::Cleared;
use crate::AmendError;

/// The adapter's client, behind a gate that takes cleared orders only.
///
/// An order that has not been through risk does not compile:
///
/// ```compile_fail,E0308
/// use fansipan_execution_engine::gate::Gate;
/// use model::SubmitOrder;
///
/// fn send(gate: &mut Gate, order: SubmitOrder) {
///     let _ = gate.submit(order);
/// }
/// ```
///
/// Nor does dressing one up as cleared outside risk:
///
/// ```compile_fail,E0423
/// use fansipan_execution_engine::risk::Cleared;
/// use model::SubmitOrder;
///
/// fn dress(order: SubmitOrder) -> Cleared<SubmitOrder> {
///     Cleared(order)
/// }
/// ```
///
/// Nor does sending one clearance twice:
///
/// ```compile_fail,E0382
/// use fansipan_execution_engine::gate::Gate;
/// use fansipan_execution_engine::risk::Cleared;
/// use model::SubmitOrder;
///
/// fn send(gate: &mut Gate, order: Cleared<SubmitOrder>) {
///     let _ = gate.submit(order);
///     let _ = gate.submit(order);
/// }
/// ```
///
/// A cleared one, sent once, does:
///
/// ```no_run
/// use fansipan_execution_engine::gate::Gate;
/// use fansipan_execution_engine::risk::Cleared;
/// use model::SubmitOrder;
///
/// fn send(gate: &mut Gate, order: Cleared<SubmitOrder>) {
///     let _ = gate.submit(order);
/// }
/// ```
pub struct Gate {
    client: Box<dyn ExecutionClient>,
}

impl Gate {
    pub(crate) fn new(client: Box<dyn ExecutionClient>) -> Self {
        Self { client }
    }

    pub(crate) fn connect(&mut self, bus: Box<dyn BusHandle>) -> Result<(), AdapterError> {
        self.client.connect(bus)
    }

    pub fn submit(&mut self, order: Cleared<SubmitOrder>) -> Result<(), AdapterError> {
        self.client.submit_order(order.into_inner())
    }

    pub fn amend(&mut self, amendment: Cleared<AmendOrder>) -> Result<(), AmendError> {
        self.client
            .amender()
            .ok_or(AmendError::Unsupported)?
            .amend_order(amendment.into_inner())
            .map_err(AmendError::Adapter)
    }

    /// A cancel takes nothing on, so it needs no clearance.
    pub(crate) fn cancel(&mut self, cancel: CancelOrder) -> Result<(), AdapterError> {
        self.client.cancel_order(cancel)
    }

    pub(crate) fn can_amend(&mut self) -> bool {
        self.client.amender().is_some()
    }

    pub(crate) fn adopt(
        &mut self,
        venue_order_id: &model::VenueOrderId,
        order_id: model::ClientOrderId,
    ) {
        self.client.adopt_order(venue_order_id, order_id);
    }

    pub(crate) fn account_id(&self) -> Option<String> {
        self.client.account_id()
    }

    /// Asking about orders takes nothing on either. `None` when the venue
    /// cannot say.
    pub(crate) fn recover(
        &mut self,
        orders: Vec<RecoverOrder>,
    ) -> Option<Result<(), AdapterError>> {
        self.client
            .recovery()
            .map(|recovery| recovery.recover_orders(orders))
    }
}
