//! Owns order state. Risk runs before anything reaches the adapter; adapters
//! only report what the venue said, and this engine decides what it means.
//! The state machine itself is in [`lifecycle`]; see /docs/order-lifecycle.md.

pub mod lifecycle;
pub mod risk;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;

use adapter_api::{AdapterError, ExecutionClient};
use bus::BusHandle;
use model::{
    CancelOrder, ClientOrderId, Connection, ConnectionStatus, Decimal, ExecutionReport, FillId,
    InstrumentId, InstrumentInfo, NewOrder, Order, OrderEvent, OrderStatus, PriceBand,
    SessionStatus, SubmitOrder,
};

pub use lifecycle::TransitionError;
pub use risk::{RiskLimits, RiskReject};

#[derive(Debug, Clone, PartialEq)]
pub enum CancelError {
    UnknownOrder(ClientOrderId),
    /// Already filled, cancelled, rejected, or denied.
    NotOpen(OrderStatus),
    Adapter(AdapterError),
}

impl fmt::Display for CancelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownOrder(id) => write!(f, "no order {id} in this session"),
            Self::NotOpen(status) => write!(f, "order is {status:?}, nothing to cancel"),
            Self::Adapter(e) => write!(f, "cancel not sent: {e}"),
        }
    }
}

impl std::error::Error for CancelError {}

/// Why an execution report changed nothing.
#[derive(Debug, Clone, PartialEq)]
pub enum ApplyError {
    /// A report for an order this engine did not create, e.g. one placed from
    /// another terminal. Adopting those needs reconciliation, which this
    /// engine does not do yet.
    UnknownOrder(ClientOrderId),
    /// A fill already applied, typically replayed after a reconnect.
    DuplicateFill(FillId),
    Transition(TransitionError),
}

impl fmt::Display for ApplyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownOrder(id) => write!(f, "report for unknown order {id}"),
            Self::DuplicateFill(fill) => write!(f, "fill {} already applied", fill.0),
            Self::Transition(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for ApplyError {}

struct Tracked {
    order: Order,
    applied_fills: HashSet<FillId>,
}

pub struct ExecutionEngine {
    client: Box<dyn ExecutionClient>,
    limits: RiskLimits,
    orders: BTreeMap<ClientOrderId, Tracked>,
    next_id: u64,
    /// Market rules as the adapter published them, for risk.
    instruments: HashMap<InstrumentId, InstrumentInfo>,
    bands: HashMap<InstrumentId, PriceBand>,
    /// Each market's phase of the day and the order types it takes, by the
    /// market's name as `InstrumentInfo::exchange` gives it.
    sessions: HashMap<String, SessionStatus>,
    connection: Option<ConnectionStatus>,
}

impl ExecutionEngine {
    pub fn new(client: Box<dyn ExecutionClient>, limits: RiskLimits) -> Self {
        Self {
            client,
            limits,
            orders: BTreeMap::new(),
            next_id: 1,
            instruments: HashMap::new(),
            bands: HashMap::new(),
            sessions: HashMap::new(),
            connection: None,
        }
    }

    /// Records an instrument's lot and tick rules for risk to apply.
    pub fn set_instrument(&mut self, info: InstrumentInfo) {
        self.instruments.insert(info.id.clone(), info);
    }

    /// Records today's price band for risk to apply.
    pub fn set_band(&mut self, band: PriceBand) {
        self.bands.insert(band.instrument.clone(), band);
    }

    /// Records a market's current session for risk to apply.
    pub fn set_session(&mut self, session: SessionStatus) {
        self.sessions.insert(session.market.clone(), session);
    }

    /// A connect that succeeds means up (the contract), so a report from
    /// before it, such as the `Down` of an earlier disconnect, no longer
    /// holds; nor do the sessions of the last connection, which the adapter
    /// publishes again.
    pub fn connect(&mut self, bus: Box<dyn BusHandle>) -> Result<(), AdapterError> {
        self.client.connect(bus)?;
        self.connection = None;
        self.sessions.clear();
        Ok(())
    }

    /// Creates an order, runs risk, and hands it to the adapter if risk
    /// clears it. Always returns the order as recorded: `Submitted` if the
    /// adapter took it, `Denied` if risk refused it, `Rejected` if the
    /// adapter refused it on the spot. The venue's answer comes later,
    /// through [`ExecutionEngine::apply`].
    #[must_use]
    pub fn submit(&mut self, request: NewOrder) -> Order {
        let id = ClientOrderId(self.next_id);
        self.next_id += 1;

        let pending = Order {
            id,
            venue_order_id: None,
            instrument: request.instrument.clone(),
            side: request.side,
            quantity: request.quantity,
            order_type: request.order_type,
            limit_price: request.limit_price,
            status: OrderStatus::Submitted,
            filled_quantity: Decimal::ZERO,
            avg_fill_price: None,
            reason: None,
        };

        let info = self.instruments.get(&request.instrument);
        let cleared = risk::check(&request, &self.limits)
            .and_then(|()| {
                risk::check_market_rules(&request, info, self.bands.get(&request.instrument))
            })
            .and_then(|()| {
                let session = info.and_then(|info| self.sessions.get(&info.exchange));
                risk::check_session(&request, session)
            });
        let order = match cleared {
            Err(reject) => Order {
                status: OrderStatus::Denied,
                reason: Some(reject.to_string()),
                ..pending
            },
            Ok(()) => match self.client.submit_order(SubmitOrder::new(id, request)) {
                Ok(()) => pending,
                Err(e) => Order {
                    status: OrderStatus::Rejected,
                    reason: Some(e.message),
                    ..pending
                },
            },
        };

        self.orders.insert(
            id,
            Tracked {
                order: order.clone(),
                applied_fills: HashSet::new(),
            },
        );
        order
    }

    /// Asks the adapter to cancel. Cancelling an order that already has a
    /// cancel in flight is a no-op that returns the order unchanged.
    pub fn cancel(&mut self, id: ClientOrderId) -> Result<Order, CancelError> {
        let tracked = self
            .orders
            .get_mut(&id)
            .ok_or(CancelError::UnknownOrder(id))?;
        let order = &tracked.order;
        if !order.status.is_open() {
            return Err(CancelError::NotOpen(order.status));
        }
        if order.status == OrderStatus::PendingCancel {
            return Ok(order.clone());
        }

        self.client
            .cancel_order(CancelOrder::new(
                id,
                order.venue_order_id.clone(),
                order.instrument.clone(),
            ))
            .map_err(CancelError::Adapter)?;

        tracked.order = Order {
            status: OrderStatus::PendingCancel,
            ..tracked.order.clone()
        };
        Ok(tracked.order.clone())
    }

    /// Applies one venue report. On error the order is left exactly as it was.
    pub fn apply(&mut self, report: &ExecutionReport) -> Result<Order, ApplyError> {
        let tracked = self
            .orders
            .get_mut(&report.order_id)
            .ok_or(ApplyError::UnknownOrder(report.order_id))?;

        if let OrderEvent::Filled { fill_id, .. } = &report.event {
            if tracked.applied_fills.contains(fill_id) {
                return Err(ApplyError::DuplicateFill(fill_id.clone()));
            }
        }

        let next =
            lifecycle::apply(&tracked.order, &report.event).map_err(ApplyError::Transition)?;
        if let OrderEvent::Filled { fill_id, .. } = &report.event {
            tracked.applied_fills.insert(fill_id.clone());
        }
        tracked.order = next;
        Ok(tracked.order.clone())
    }

    pub fn order(&self, id: ClientOrderId) -> Option<&Order> {
        self.orders.get(&id).map(|t| &t.order)
    }

    /// Every order this session, oldest first.
    pub fn orders(&self) -> impl Iterator<Item = &Order> {
        self.orders.values().map(|t| &t.order)
    }

    /// Records a report on the orders connection; returns whether kept.
    /// Reports on the adapter's other connections are not this engine's.
    pub fn on_connection(&mut self, status: &ConnectionStatus) -> bool {
        if status.connection != Connection::Orders {
            return false;
        }
        self.connection = Some(status.clone());
        true
    }

    /// The orders connection as last reported. `None` means no report since
    /// `connect` succeeded, which the contract defines as up.
    pub fn connection(&self) -> Option<&ConnectionStatus> {
        self.connection.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use model::{AccountState, ConnectionState, InstrumentId, Side, UnixNanos, VenueOrderId};
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct Calls {
        submitted: Vec<SubmitOrder>,
        cancelled: Vec<CancelOrder>,
    }

    struct RecordingClient {
        calls: Arc<Mutex<Calls>>,
        refuse_with: Option<AdapterError>,
    }

    impl ExecutionClient for RecordingClient {
        fn connect(&mut self, _bus: Box<dyn BusHandle>) -> Result<(), AdapterError> {
            Ok(())
        }
        fn submit_order(&mut self, order: SubmitOrder) -> Result<(), AdapterError> {
            if let Some(e) = &self.refuse_with {
                return Err(e.clone());
            }
            self.calls.lock().unwrap().submitted.push(order);
            Ok(())
        }
        fn cancel_order(&mut self, cancel: CancelOrder) -> Result<(), AdapterError> {
            if let Some(e) = &self.refuse_with {
                return Err(e.clone());
            }
            self.calls.lock().unwrap().cancelled.push(cancel);
            Ok(())
        }
        fn account_state(&mut self) -> Result<AccountState, AdapterError> {
            Ok(AccountState::default())
        }
        fn disconnect(&mut self) -> Result<(), AdapterError> {
            Ok(())
        }
    }

    fn d(raw: &str) -> Decimal {
        raw.parse().expect("test decimal")
    }

    fn engine_with(refuse_with: Option<AdapterError>) -> (ExecutionEngine, Arc<Mutex<Calls>>) {
        let calls = Arc::new(Mutex::new(Calls::default()));
        let client = RecordingClient {
            calls: calls.clone(),
            refuse_with,
        };
        let limits = RiskLimits {
            max_order_quantity: d("10"),
            max_order_notional: d("1000"),
        };
        (ExecutionEngine::new(Box::new(client), limits), calls)
    }

    fn engine() -> (ExecutionEngine, Arc<Mutex<Calls>>) {
        engine_with(None)
    }

    fn buy(quantity: &str) -> NewOrder {
        NewOrder::limit(
            InstrumentId::new("sim", "AAA"),
            Side::Buy,
            d(quantity),
            d("5"),
        )
    }

    fn report(id: ClientOrderId, event: OrderEvent) -> ExecutionReport {
        ExecutionReport {
            order_id: id,
            event,
            ts_event: UnixNanos(0),
        }
    }

    fn fill(fill_id: &str, quantity: &str) -> OrderEvent {
        OrderEvent::Filled {
            fill_id: FillId(fill_id.into()),
            quantity: d(quantity),
            price: d("5"),
        }
    }

    #[test]
    fn risk_denial_is_recorded_and_never_reaches_the_adapter() {
        let (mut engine, calls) = engine();

        let order = engine.submit(buy("11"));

        assert_eq!(order.status, OrderStatus::Denied);
        assert_eq!(
            order.reason.as_deref(),
            Some("quantity is above the per-order limit of 10")
        );
        assert!(calls.lock().unwrap().submitted.is_empty());
        assert_eq!(engine.orders().count(), 1);
    }

    #[test]
    fn market_rules_deny_an_odd_lot_once_the_instrument_is_known() {
        let (mut engine, calls) = engine();
        let ticks = model::TickTable::flat(d("0.01")).unwrap();
        engine.set_instrument(InstrumentInfo::new(buy("1").instrument, d("3"), ticks));

        let order = engine.submit(buy("4"));

        assert_eq!(order.status, OrderStatus::Denied);
        assert_eq!(
            order.reason.as_deref(),
            Some("quantity must be a multiple of the lot size, 3")
        );
        assert!(calls.lock().unwrap().submitted.is_empty());
    }

    #[test]
    fn market_rules_deny_a_limit_outside_the_band() {
        let (mut engine, _) = engine();
        engine.set_band(PriceBand::new(
            buy("1").instrument,
            d("5"),
            Some(d("4.5")),
            Some(d("1")),
            UnixNanos(0),
        ));

        let order = engine.submit(buy("2"));

        assert_eq!(order.status, OrderStatus::Denied);
    }

    #[test]
    fn cleared_order_reaches_the_adapter_carrying_the_engine_id() {
        let (mut engine, calls) = engine();

        let order = engine.submit(buy("3"));

        assert_eq!(order.status, OrderStatus::Submitted);
        assert_eq!(calls.lock().unwrap().submitted[0].order_id, order.id);
    }

    #[test]
    fn each_order_gets_a_fresh_id() {
        let (mut engine, _) = engine();

        let a = engine.submit(buy("1"));
        let b = engine.submit(buy("1"));

        assert_ne!(a.id, b.id);
    }

    #[test]
    fn adapter_refusing_on_the_spot_rejects_the_order() {
        let (mut engine, _) = engine_with(Some(AdapterError::recoverable("socket closed")));

        let order = engine.submit(buy("1"));

        assert_eq!(order.status, OrderStatus::Rejected);
        assert_eq!(order.reason.as_deref(), Some("socket closed"));
    }

    #[test]
    fn reports_drive_the_order_to_filled() {
        let (mut engine, _) = engine();
        let id = engine.submit(buy("4")).id;

        engine
            .apply(&report(
                id,
                OrderEvent::Accepted {
                    venue_order_id: VenueOrderId("V9".into()),
                },
            ))
            .unwrap();
        engine.apply(&report(id, fill("f1", "1"))).unwrap();
        let done = engine.apply(&report(id, fill("f2", "3"))).unwrap();

        assert_eq!(done.status, OrderStatus::Filled);
        assert_eq!(engine.order(id).map(|o| o.filled_quantity), Some(d("4")));
    }

    #[test]
    fn a_replayed_fill_is_dropped() {
        let (mut engine, _) = engine();
        let id = engine.submit(buy("4")).id;
        engine.apply(&report(id, fill("f1", "1"))).unwrap();

        let replay = engine.apply(&report(id, fill("f1", "1")));

        assert_eq!(replay, Err(ApplyError::DuplicateFill(FillId("f1".into()))));
        assert_eq!(engine.order(id).map(|o| o.filled_quantity), Some(d("1")));
    }

    #[test]
    fn a_rejected_fill_does_not_burn_its_fill_id() {
        let (mut engine, _) = engine();
        let id = engine.submit(buy("4")).id;
        assert!(engine.apply(&report(id, fill("f1", "9"))).is_err());

        let retry = engine.apply(&report(id, fill("f1", "1")));

        assert!(retry.is_ok());
    }

    #[test]
    fn report_for_an_unknown_order_is_refused() {
        let (mut engine, _) = engine();

        let result = engine.apply(&report(ClientOrderId(99), OrderEvent::Cancelled));

        assert_eq!(result, Err(ApplyError::UnknownOrder(ClientOrderId(99))));
    }

    #[test]
    fn cancel_sends_the_venue_id_and_marks_the_order_pending() {
        let (mut engine, calls) = engine();
        let id = engine.submit(buy("4")).id;
        engine
            .apply(&report(
                id,
                OrderEvent::Accepted {
                    venue_order_id: VenueOrderId("V9".into()),
                },
            ))
            .unwrap();

        let order = engine.cancel(id).unwrap();

        assert_eq!(order.status, OrderStatus::PendingCancel);
        assert_eq!(
            calls.lock().unwrap().cancelled[0].venue_order_id,
            Some(VenueOrderId("V9".into()))
        );
    }

    #[test]
    fn a_second_cancel_while_one_is_in_flight_is_not_resent() {
        let (mut engine, calls) = engine();
        let id = engine.submit(buy("4")).id;
        engine.cancel(id).unwrap();

        let again = engine.cancel(id).unwrap();

        assert_eq!(again.status, OrderStatus::PendingCancel);
        assert_eq!(calls.lock().unwrap().cancelled.len(), 1);
    }

    #[test]
    fn a_finished_order_cannot_be_cancelled() {
        let (mut engine, _) = engine();
        let id = engine.submit(buy("4")).id;
        engine.apply(&report(id, fill("f1", "4"))).unwrap();

        assert_eq!(
            engine.cancel(id),
            Err(CancelError::NotOpen(OrderStatus::Filled))
        );
    }

    #[test]
    fn a_cancel_the_adapter_cannot_send_leaves_the_order_working() {
        let (mut engine, calls) = engine();
        let id = engine.submit(buy("4")).id;
        engine.client = Box::new(RecordingClient {
            calls,
            refuse_with: Some(AdapterError::recoverable("socket closed")),
        });

        let result = engine.cancel(id);

        assert!(matches!(result, Err(CancelError::Adapter(_))));
        assert_eq!(
            engine.order(id).map(|o| o.status),
            Some(OrderStatus::Submitted)
        );
    }

    #[test]
    fn keeps_the_latest_orders_connection_report_and_no_other() {
        let (mut engine, _) = engine();
        assert_eq!(engine.connection(), None);
        let down = ConnectionStatus::new(Connection::Orders, ConnectionState::Down, UnixNanos(1))
            .with_detail("keys refused");
        let data = ConnectionStatus::new(
            Connection::MarketData,
            ConnectionState::Degraded,
            UnixNanos(2),
        );

        assert!(engine.on_connection(&down));
        assert!(!engine.on_connection(&data));

        assert_eq!(engine.connection(), Some(&down));
    }

    #[test]
    fn connecting_again_forgets_the_last_report() {
        let (mut engine, _) = engine();
        let down = ConnectionStatus::new(Connection::Orders, ConnectionState::Down, UnixNanos(1));
        engine.on_connection(&down);

        engine.connect(bus::Bus::new().handle()).unwrap();

        assert_eq!(engine.connection(), None);
    }

    #[test]
    fn a_type_the_market_does_not_take_now_never_reaches_the_adapter() {
        let (mut engine, calls) = engine();
        let ticks = model::TickTable::flat(d("0.01")).unwrap();
        engine.set_instrument(
            InstrumentInfo::new(InstrumentId::new("sim", "AAA"), Decimal::ONE, ticks)
                .with_exchange("HOSE"),
        );
        engine.set_session(model::SessionStatus::new(
            "HOSE",
            model::SessionPhase::Break,
            UnixNanos(0),
        ));

        let order = engine.submit(buy("1"));

        assert_eq!(order.status, OrderStatus::Denied);
        assert_eq!(
            order.reason.as_deref(),
            Some("HOSE is in a break and takes no orders now")
        );
        assert!(calls.lock().unwrap().submitted.is_empty());
    }

    #[test]
    fn connecting_again_forgets_the_last_connections_sessions() {
        let (mut engine, calls) = engine();
        let ticks = model::TickTable::flat(d("0.01")).unwrap();
        engine.set_instrument(
            InstrumentInfo::new(InstrumentId::new("sim", "AAA"), Decimal::ONE, ticks)
                .with_exchange("HOSE"),
        );
        engine.set_session(model::SessionStatus::new(
            "HOSE",
            model::SessionPhase::Closed,
            UnixNanos(0),
        ));

        engine.connect(bus::Bus::new().handle()).unwrap();
        let order = engine.submit(buy("1"));

        assert_eq!(order.status, OrderStatus::Submitted);
        assert_eq!(calls.lock().unwrap().submitted.len(), 1);
    }
}
