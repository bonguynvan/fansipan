//! Owns order state and the account as the venue last reported it. Risk
//! runs before anything reaches the adapter; adapters only report what the
//! venue said, and this engine decides what it means.
//! The state machine itself is in [`lifecycle`]; see /docs/order-lifecycle.md.

pub mod capacity;
pub mod gate;
pub mod lifecycle;
pub mod pnl;
pub mod risk;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;

use adapter_api::{AdapterError, ExecutionClient};
use bus::BusHandle;
use model::{
    AccountState, CancelOrder, ClientOrderId, Connection, ConnectionStatus, Decimal,
    ExecutionReport, FillId, InstrumentId, InstrumentInfo, NewOrder, Order, OrderEvent,
    OrderStatus, PriceBand, SessionStatus, Side,
};
use model::{OrderKind, OrderType, VenueOrder, VenueOrderState};

pub use capacity::{Bound, BuyingPower, OrderCapacity};
pub use gate::Gate;
pub use lifecycle::TransitionError;
pub use pnl::DayPnl;
pub use risk::{Cleared, RiskLimits, RiskReject, TradingState};

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

/// Why an amendment was not sent. The order stands as it was.
#[derive(Debug, Clone, PartialEq)]
pub enum AmendError {
    UnknownOrder(ClientOrderId),
    /// Not resting on the venue, or a cancel or another amendment is in
    /// flight.
    NotAmendable(OrderStatus),
    /// Only a limit order has a price to change.
    NotALimitOrder,
    /// The new quantity is not above what has already filled.
    BelowFilled(Decimal),
    /// The adapter's venue cannot amend orders.
    Unsupported,
    /// Risk refused the new terms.
    Denied(RiskReject),
    Adapter(AdapterError),
}

impl fmt::Display for AmendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownOrder(id) => write!(f, "no order {id} in this session"),
            Self::NotAmendable(status) => {
                write!(f, "order is {status:?}, it cannot be amended now")
            }
            Self::NotALimitOrder => f.write_str("only a limit order can be amended"),
            Self::BelowFilled(filled) => {
                write!(
                    f,
                    "the new quantity must be above the {filled} already filled"
                )
            }
            Self::Unsupported => f.write_str("this venue does not amend orders"),
            Self::Denied(reject) => write!(f, "denied by risk: {reject}"),
            Self::Adapter(e) => write!(f, "amendment not sent: {e}"),
        }
    }
}

impl std::error::Error for AmendError {}

/// Whether an amendment could change `order`: a limit order resting on the
/// venue, with no cancel or other amendment in flight. Whether the venue
/// amends at all is [`ExecutionEngine::can_amend`].
pub fn is_amendable(order: &Order) -> bool {
    amend_refusal(order).is_none()
}

fn amend_refusal(order: &Order) -> Option<AmendError> {
    if !matches!(
        order.status,
        OrderStatus::Accepted | OrderStatus::PartiallyFilled
    ) {
        return Some(AmendError::NotAmendable(order.status));
    }
    if order.order_type.kind != OrderKind::Limit {
        return Some(AmendError::NotALimitOrder);
    }
    None
}

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
    /// The total quantity an amendment in flight asks for.
    amending_to: Option<Decimal>,
}

impl Tracked {
    /// What the order may still sell or buy: its remaining quantity, or more
    /// while an amendment for more is in flight, since the venue may take it.
    fn exposure(&self) -> Decimal {
        let remaining = self.order.remaining();
        self.amending_to.map_or(remaining, |to| {
            remaining.max(to - self.order.filled_quantity)
        })
    }
}

pub struct ExecutionEngine {
    gate: Gate,
    trading: TradingState,
    limits: RiskLimits,
    orders: BTreeMap<ClientOrderId, Tracked>,
    next_id: u64,
    /// Market rules as the adapter published them, for risk.
    instruments: HashMap<InstrumentId, InstrumentInfo>,
    bands: HashMap<InstrumentId, PriceBand>,
    /// Each market's phase of the day and the order types it takes, by the
    /// market's name as `InstrumentInfo::exchange` gives it.
    sessions: HashMap<String, SessionStatus>,
    /// Cash and positions as the venue last reported them.
    account: AccountState,
    connection: Option<ConnectionStatus>,
}

impl ExecutionEngine {
    pub fn new(client: Box<dyn ExecutionClient>, limits: RiskLimits) -> Self {
        Self {
            gate: Gate::new(client),
            trading: TradingState::Active,
            limits,
            orders: BTreeMap::new(),
            next_id: 1,
            instruments: HashMap::new(),
            bands: HashMap::new(),
            sessions: HashMap::new(),
            account: AccountState::default(),
            connection: None,
        }
    }

    /// The day's profit and loss (ADR 0019) for every instrument held now
    /// or traded this session, by instrument. `mark` gives an instrument's
    /// price now, or `None` when it has none.
    pub fn day_pnl(&self, mark: impl Fn(&InstrumentId) -> Option<Decimal>) -> Vec<DayPnl> {
        self.day_flows()
            .into_iter()
            .map(|(instrument, flows)| {
                let held = self
                    .account
                    .position(&instrument)
                    .map_or(Decimal::ZERO, |p| p.quantity);
                let reference = self.bands.get(&instrument).map(|band| band.reference);
                let costs = self
                    .instruments
                    .get(&instrument)
                    .map(|info| info.costs)
                    .unwrap_or_default();
                let (pnl, costs) = pnl::day_pnl(&flows, held, reference, mark(&instrument), costs);
                DayPnl {
                    instrument,
                    pnl,
                    costs,
                }
            })
            .collect()
    }

    /// The instruments the day counts: held now or traded this session.
    /// The shell prices only these.
    pub fn day_instruments(&self) -> Vec<InstrumentId> {
        self.day_flows().into_keys().collect()
    }

    fn day_flows(&self) -> BTreeMap<InstrumentId, pnl::Flows> {
        let mut flows: BTreeMap<InstrumentId, pnl::Flows> = BTreeMap::new();
        for tracked in self.orders.values() {
            let order = &tracked.order;
            if order.filled_quantity.is_zero() {
                continue;
            }
            let entry = flows.entry(order.instrument.clone()).or_default();
            *entry = entry.add(
                order.side == Side::Buy,
                order.filled_quantity,
                order.avg_fill_price,
            );
        }
        for position in &self.account.positions {
            flows.entry(position.instrument.clone()).or_default();
        }
        flows
    }

    /// Whether this terminal sends new orders now (risk's first check).
    pub fn trading_state(&self) -> TradingState {
        self.trading
    }

    /// Halts trading, limits it to orders that reduce a position, or lets
    /// it run again. Returns whether the state changed. Orders already
    /// working stay working: halting cancels nothing by itself.
    pub fn set_trading_state(&mut self, state: TradingState) -> bool {
        let changed = self.trading != state;
        self.trading = state;
        changed
    }

    /// Records an instrument's lot and tick rules for risk to apply.
    pub fn set_instrument(&mut self, info: InstrumentInfo) {
        self.instruments.insert(info.id.clone(), info);
    }

    /// Records today's price band for risk to apply.
    pub fn set_band(&mut self, band: PriceBand) {
        self.bands.insert(band.instrument.clone(), band);
    }

    /// Records the account as the venue reported it, for risk to check sells
    /// against.
    pub fn set_account(&mut self, account: AccountState) {
        self.account = account;
    }

    pub fn account(&self) -> &AccountState {
        &self.account
    }

    /// How large one order in `instrument` could be now (ADR 0016): what
    /// the account can pay for or holds, and this terminal's limits, in the
    /// quantities the market takes. Risk still judges the order itself.
    pub fn capacity(
        &self,
        instrument: &InstrumentId,
        side: Side,
        order_type: OrderType,
        limit_price: Option<Decimal>,
    ) -> OrderCapacity {
        capacity::capacity(&capacity::Inputs {
            side,
            order_type,
            limit_price,
            info: self.instruments.get(instrument),
            band: self.bands.get(instrument),
            account: &self.account,
            position: self.account.position(instrument),
            working_buys: self.working_buys_value(),
            working_sells: self.working_sells(instrument),
            limits: &self.limits,
        })
    }

    /// What this session's working buys in every instrument may still
    /// spend: at their limit, or at the day's ceiling for one without. A buy
    /// with neither cannot be valued and is left out.
    fn working_buys_value(&self) -> Decimal {
        self.orders
            .values()
            .filter(|t| t.order.side == Side::Buy && t.order.status.is_open())
            .filter_map(|t| {
                let price = t.order.limit_price.or_else(|| {
                    self.bands
                        .get(&t.order.instrument)
                        .and_then(|band| band.ceiling)
                })?;
                t.exposure().checked_mul(price)
            })
            .fold(Decimal::ZERO, |sum, value| {
                sum.checked_add(value).unwrap_or(Decimal::MAX)
            })
    }

    /// What this session's working sell orders in `instrument` have yet to
    /// sell.
    fn working_sells(&self, instrument: &InstrumentId) -> Decimal {
        self.working(instrument, Side::Sell)
    }

    /// What the open orders on `side` in `instrument` may still trade.
    fn working(&self, instrument: &InstrumentId, side: Side) -> Decimal {
        self.orders
            .values()
            .filter(|t| {
                let o = &t.order;
                &o.instrument == instrument && o.side == side && o.status.is_open()
            })
            .map(Tracked::exposure)
            // Saturating, not overflowing: sizes from outside cannot be
            // trusted to add up, and a total stuck at the maximum only
            // makes risk refuse more sells, never fewer.
            .fold(Decimal::ZERO, |sum, exposure| {
                sum.checked_add(exposure).unwrap_or(Decimal::MAX)
            })
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
        self.gate.connect(bus)?;
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

        let order = match self.clear(request, None) {
            Err(reject) => Order {
                status: OrderStatus::Denied,
                reason: Some(reject.to_string()),
                ..pending
            },
            Ok(cleared) => match self.gate.submit(cleared.submit(id)) {
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
                amending_to: None,
            },
        );
        order
    }

    /// Runs risk on `terms` against what this engine knows. For an
    /// amendment, `amending` is the order as it stands: the sell check then
    /// counts only the shares the new terms leave to sell, against working
    /// sells other than this order.
    fn clear(
        &self,
        terms: NewOrder,
        amending: Option<&Order>,
    ) -> Result<Cleared<NewOrder>, RiskReject> {
        let info = self.instruments.get(&terms.instrument);
        // Working orders other than the one being amended, which its new
        // terms replace.
        let others = |side: Side| {
            let all = self.working(&terms.instrument, side);
            amending
                .filter(|order| order.side == side)
                .map_or(all, |order| {
                    all.checked_sub(order.remaining()).unwrap_or(Decimal::ZERO)
                })
        };
        let filled = amending.map_or(Decimal::ZERO, |order| order.filled_quantity);
        let context = risk::Context {
            trading: self.trading,
            adds: amending.is_none_or(|order| terms.quantity > order.quantity),
            limits: &self.limits,
            info,
            band: self.bands.get(&terms.instrument),
            session: info.and_then(|info| self.sessions.get(&info.exchange)),
            position: self.account.position(&terms.instrument),
            working_sells: others(Side::Sell),
            working_buys: others(Side::Buy),
            filled,
        };
        risk::clear(terms, &context)
    }

    /// Asks the adapter to change a resting limit order to `quantity` (its
    /// new total; what has filled stays filled) at `limit_price`. The new
    /// terms pass every risk check a new order would. On success the order
    /// is `PendingAmend` until the venue answers; on any error it stands as
    /// it was.
    pub fn amend(
        &mut self,
        id: ClientOrderId,
        quantity: Decimal,
        limit_price: Decimal,
    ) -> Result<Order, AmendError> {
        let current = self
            .orders
            .get(&id)
            .map(|t| t.order.clone())
            .ok_or(AmendError::UnknownOrder(id))?;
        if let Some(refusal) = amend_refusal(&current) {
            return Err(refusal);
        }
        if quantity <= current.filled_quantity {
            return Err(AmendError::BelowFilled(current.filled_quantity));
        }
        if !self.gate.can_amend() {
            return Err(AmendError::Unsupported);
        }
        let terms = NewOrder::new(
            current.instrument.clone(),
            current.side,
            quantity,
            current.order_type,
            Some(limit_price),
        );
        let cleared = self
            .clear(terms, Some(&current))
            .map_err(AmendError::Denied)?;
        self.gate
            .amend(cleared.amend(id, current.venue_order_id.clone()))?;

        let pending = Order {
            status: OrderStatus::PendingAmend,
            ..current
        };
        if let Some(tracked) = self.orders.get_mut(&id) {
            tracked.order = pending.clone();
            tracked.amending_to = Some(quantity);
        }
        Ok(pending)
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

        self.gate
            .cancel(CancelOrder::new(
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

    /// Asks the adapter to cancel every open order, or every open order in
    /// `instrument`. An order already being cancelled is not asked again.
    /// Each order's outcome is its own: one the adapter cannot cancel stays
    /// working, and the rest still go.
    pub fn cancel_all(
        &mut self,
        instrument: Option<&InstrumentId>,
    ) -> Vec<(ClientOrderId, Result<Order, CancelError>)> {
        let ids: Vec<ClientOrderId> = self
            .orders
            .values()
            .map(|tracked| &tracked.order)
            .filter(|order| order.status.is_open() && order.status != OrderStatus::PendingCancel)
            .filter(|order| instrument.is_none_or(|wanted| &order.instrument == wanted))
            .map(|order| order.id)
            .collect();
        ids.into_iter().map(|id| (id, self.cancel(id))).collect()
    }

    /// Applies one venue report. On error the order is left exactly as it was.
    pub fn apply(&mut self, report: &ExecutionReport) -> Result<Order, ApplyError> {
        if let OrderEvent::Snapshot(venue) = &report.event {
            return self.catch_up(report.order_id, venue);
        }
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
        if next.status != OrderStatus::PendingAmend {
            tracked.amending_to = None;
        }
        tracked.order = next;
        Ok(tracked.order.clone())
    }

    /// Catches an order up with the venue's view of it, read back after a
    /// reconnect, in order: an acknowledgement the stream missed; the
    /// venue's terms, which settle an amendment whose answer was missed;
    /// the fills it missed, as one fill priced so the order's average
    /// matches the venue's; and the venue's end of the order. The missed
    /// fill's id is the cumulative quantity it brings the order to, so the
    /// same view twice changes nothing. A transition that does not fit
    /// leaves the order as it was. Missed fills the venue gives no price for
    /// are not invented: the rest still applies, and the order's reason
    /// says so.
    fn catch_up(&mut self, id: ClientOrderId, venue: &VenueOrder) -> Result<Order, ApplyError> {
        let tracked = self.orders.get(&id).ok_or(ApplyError::UnknownOrder(id))?;
        let step = |order: &Order, event: OrderEvent| {
            lifecycle::apply(order, &event).map_err(ApplyError::Transition)
        };
        let mut next = tracked.order.clone();
        let mut caught_up_fill = None;

        if let (None, Some(venue_order_id)) = (&next.venue_order_id, &venue.venue_order_id) {
            next = step(
                &next,
                OrderEvent::Accepted {
                    venue_order_id: venue_order_id.clone(),
                },
            )?;
        }
        if let Some(quantity) = venue.quantity {
            let changed = quantity != next.quantity || venue.limit_price != next.limit_price;
            if next.status == OrderStatus::PendingAmend || (changed && next.status.is_open()) {
                next = step(
                    &next,
                    OrderEvent::Amended {
                        quantity,
                        limit_price: venue.limit_price,
                    },
                )?;
            }
        }
        if venue.filled_quantity > next.filled_quantity {
            let fill_id = FillId(format!("catch-up:{id}:{}", venue.filled_quantity));
            let missed = venue.filled_quantity - next.filled_quantity;
            match catch_up_price(&next, venue) {
                Some(_) if tracked.applied_fills.contains(&fill_id) => {}
                Some(price) => {
                    next = step(
                        &next,
                        OrderEvent::Filled {
                            fill_id: fill_id.clone(),
                            quantity: missed,
                            price,
                        },
                    )?;
                    caught_up_fill = Some(fill_id);
                }
                None => {
                    next.reason = Some(format!(
                        "the venue reports {missed} more filled with no price to record it at"
                    ))
                }
            }
        }
        let end = match &venue.state {
            VenueOrderState::Cancelled => Some(OrderEvent::Cancelled),
            VenueOrderState::Rejected { reason } if next.filled_quantity.is_zero() => {
                Some(OrderEvent::Rejected {
                    reason: reason.clone(),
                })
            }
            // Fills happened, so the order cannot be rejected as a whole.
            VenueOrderState::Rejected { .. } => Some(OrderEvent::Cancelled),
            // Filled at fewer than core's quantity: the venue's total is what
            // traded, as after an amendment down whose answer was missed.
            VenueOrderState::Filled if next.filled_quantity == venue.filled_quantity => {
                Some(OrderEvent::Amended {
                    quantity: next.filled_quantity,
                    limit_price: next.limit_price,
                })
            }
            _ => None,
        };
        if let Some(end) = end.filter(|_| next.status.is_open()) {
            next = step(&next, end)?;
        }

        let tracked = self
            .orders
            .get_mut(&id)
            .ok_or(ApplyError::UnknownOrder(id))?;
        if let Some(fill_id) = caught_up_fill {
            tracked.applied_fills.insert(fill_id);
        }
        if next.status != OrderStatus::PendingAmend {
            tracked.amending_to = None;
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

    /// Whether the adapter's venue takes amendments at all. Which orders
    /// one could change is [`is_amendable`].
    pub fn can_amend(&mut self) -> bool {
        self.gate.can_amend()
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

/// The price of the fills a stream missed: the one that makes the order's
/// average match the venue's. A venue that rounds its average can make that
/// price nonsense (zero or below, for a small missed quantity); the venue's
/// average is then the honest approximation. `None` only when the venue
/// gives no average at all.
fn catch_up_price(order: &Order, venue: &VenueOrder) -> Option<Decimal> {
    let venue_avg = venue.avg_fill_price.filter(|avg| *avg > Decimal::ZERO)?;
    let exact = venue
        .filled_quantity
        .checked_sub(order.filled_quantity)
        .zip(venue_avg.checked_mul(venue.filled_quantity))
        .zip(
            order
                .avg_fill_price
                .unwrap_or_default()
                .checked_mul(order.filled_quantity),
        )
        .and_then(|((missed, venue_value), known_value)| {
            venue_value.checked_sub(known_value)?.checked_div(missed)
        });
    Some(
        exact
            .filter(|price| *price > Decimal::ZERO)
            .unwrap_or(venue_avg),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use model::{
        AccountState, AmendOrder, ConnectionState, InstrumentId, Side, SubmitOrder, UnixNanos,
        VenueOrderId,
    };
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct Calls {
        submitted: Vec<SubmitOrder>,
        cancelled: Vec<CancelOrder>,
        amended: Vec<AmendOrder>,
    }

    struct RecordingClient {
        calls: Arc<Mutex<Calls>>,
        refuse_with: Option<AdapterError>,
        amends: bool,
    }

    impl adapter_api::OrderAmender for RecordingClient {
        fn amend_order(&mut self, amend: AmendOrder) -> Result<(), AdapterError> {
            self.calls.lock().unwrap().amended.push(amend);
            Ok(())
        }
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
        fn amender(&mut self) -> Option<&mut dyn adapter_api::OrderAmender> {
            if self.amends {
                Some(self)
            } else {
                None
            }
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
            amends: true,
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
    fn capacity_counts_this_sessions_working_orders() {
        let (mut engine, _) = engine();
        let aaa = InstrumentId::new("sim", "AAA");
        engine.set_instrument(
            InstrumentInfo::new(
                aaa.clone(),
                d("1"),
                model::TickTable::flat(d("0.01")).unwrap(),
            )
            .with_short_selling(false),
        );
        engine.set_account(AccountState::new(
            d("1000"),
            vec![model::Position::new(aaa.clone(), d("8"), d("5")).with_settled(d("8"))],
        ));
        let working = [
            engine.submit(buy("4")),
            engine.submit(NewOrder::limit(aaa.clone(), Side::Sell, d("3"), d("6"))),
        ];
        assert!(working.iter().all(|order| order.status.is_open()));

        let buying = engine.capacity(&aaa, Side::Buy, model::OrderType::LIMIT, Some(d("5")));
        let selling = engine.capacity(&aaa, Side::Sell, model::OrderType::LIMIT, Some(d("6")));

        // Cash 1000 less the working buy's 4 × 5; the order-size cap of 10 binds.
        assert_eq!(
            buying.buying_power,
            BuyingPower {
                amount: d("980"),
                estimated: true
            }
        );
        assert_eq!(
            (buying.max_quantity, buying.bound),
            (Some(d("10")), Some(Bound::OrderSize))
        );
        // 8 settled, 3 already in a working sell.
        assert_eq!(
            (selling.max_quantity, selling.bound),
            (Some(d("5")), Some(Bound::Holding))
        );
    }

    #[test]
    fn capacity_prefers_the_venues_buying_power() {
        let (mut engine, _) = engine();
        let aaa = InstrumentId::new("sim", "AAA");
        engine.set_account(AccountState::new(d("1000"), vec![]).with_buying_power(d("30")));

        let buying = engine.capacity(&aaa, Side::Buy, model::OrderType::LIMIT, Some(d("5")));

        assert_eq!(
            buying.buying_power,
            BuyingPower {
                amount: d("30"),
                estimated: false
            }
        );
        assert_eq!(
            (buying.max_quantity, buying.bound),
            (Some(d("6")), Some(Bound::BuyingPower))
        );
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
        engine.gate = Gate::new(Box::new(RecordingClient {
            calls,
            refuse_with: Some(AdapterError::recoverable("socket closed")),
            amends: true,
        }));

        let result = engine.cancel(id);

        assert!(matches!(result, Err(CancelError::Adapter(_))));
        assert_eq!(
            engine.order(id).map(|o| o.status),
            Some(OrderStatus::Submitted)
        );
    }

    #[test]
    fn cancel_all_asks_for_every_open_order_once() {
        let (mut engine, calls) = engine();
        let working = resting(&mut engine, "2");
        let unacked = engine.submit(buy("1")).id;
        let cancelling = resting(&mut engine, "3");
        engine.cancel(cancelling).unwrap();
        let filled = resting(&mut engine, "1");
        engine.apply(&report(filled, fill("F1", "1"))).unwrap();
        let denied = engine.submit(buy("99")).id;
        calls.lock().unwrap().cancelled.clear();

        let outcome = engine.cancel_all(None);

        let asked: Vec<ClientOrderId> = outcome.iter().map(|(id, _)| *id).collect();
        assert_eq!(asked, vec![working, unacked]);
        assert!(outcome.iter().all(|(_, result)| result.is_ok()));
        assert_eq!(calls.lock().unwrap().cancelled.len(), 2);
        for id in [working, unacked, cancelling] {
            assert_eq!(
                engine.order(id).map(|o| o.status),
                Some(OrderStatus::PendingCancel)
            );
        }
        assert_eq!(
            engine.order(filled).map(|o| o.status),
            Some(OrderStatus::Filled)
        );
        assert_eq!(
            engine.order(denied).map(|o| o.status),
            Some(OrderStatus::Denied)
        );
    }

    #[test]
    fn cancel_all_for_one_instrument_leaves_the_others() {
        let (mut engine, _) = engine();
        let here = resting(&mut engine, "2");
        let elsewhere = engine
            .submit(NewOrder::limit(
                InstrumentId::new("sim", "BBB"),
                Side::Buy,
                d("1"),
                d("5"),
            ))
            .id;

        let outcome = engine.cancel_all(Some(&InstrumentId::new("sim", "AAA")));

        assert_eq!(outcome.len(), 1);
        assert_eq!(outcome[0].0, here);
        assert_eq!(
            engine.order(elsewhere).map(|o| o.status),
            Some(OrderStatus::Submitted)
        );
    }

    #[test]
    fn cancel_all_reports_each_order_the_adapter_could_not_cancel() {
        let (mut engine, calls) = engine();
        let first = resting(&mut engine, "1");
        let second = resting(&mut engine, "2");
        engine.gate = Gate::new(Box::new(RecordingClient {
            calls,
            refuse_with: Some(AdapterError::recoverable("socket closed")),
            amends: true,
        }));

        let outcome = engine.cancel_all(None);

        assert_eq!(outcome.len(), 2);
        assert!(outcome
            .iter()
            .all(|(_, result)| matches!(result, Err(CancelError::Adapter(_)))));
        for id in [first, second] {
            assert_eq!(
                engine.order(id).map(|o| o.status),
                Some(OrderStatus::Accepted)
            );
        }
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

    #[test]
    fn a_sell_beyond_the_settled_holding_never_reaches_the_adapter() {
        let (mut engine, calls) = engine();
        let instrument = InstrumentId::new("sim", "AAA");
        let ticks = model::TickTable::flat(d("0.01")).unwrap();
        engine.set_instrument(
            InstrumentInfo::new(instrument.clone(), Decimal::ONE, ticks).with_short_selling(false),
        );
        let held = model::Position::new(instrument.clone(), d("10"), d("5")).with_settled(d("4"));
        engine.set_account(AccountState::new(d("1000"), vec![held]));
        let sell =
            |quantity: &str| NewOrder::limit(instrument.clone(), Side::Sell, d(quantity), d("5"));

        let first = engine.submit(sell("3"));
        let second = engine.submit(sell("2"));

        assert_eq!(first.status, OrderStatus::Submitted);
        assert_eq!(second.status, OrderStatus::Denied);
        assert_eq!(
            second.reason.as_deref(),
            Some("only 1 can be sold now: 10 held, 6 not settled yet, 3 in working sell orders")
        );
        assert_eq!(calls.lock().unwrap().submitted.len(), 1);
    }

    /// An order the venue has accepted, at 5.
    fn resting(engine: &mut ExecutionEngine, quantity: &str) -> ClientOrderId {
        let id = engine.submit(buy(quantity)).id;
        engine
            .apply(&report(
                id,
                OrderEvent::Accepted {
                    venue_order_id: VenueOrderId("V1".into()),
                },
            ))
            .unwrap();
        id
    }

    #[test]
    fn an_amendment_goes_to_the_venue_and_waits_for_its_answer() {
        let (mut engine, calls) = engine();
        let id = resting(&mut engine, "2");

        let order = engine.amend(id, d("4"), d("6")).unwrap();

        assert_eq!(order.status, OrderStatus::PendingAmend);
        assert_eq!((order.quantity, order.limit_price), (d("2"), Some(d("5"))));
        let sent = &calls.lock().unwrap().amended[0];
        assert_eq!(
            (sent.order_id, sent.quantity, sent.limit_price),
            (id, d("4"), Some(d("6")))
        );
        assert_eq!(sent.venue_order_id, Some(VenueOrderId("V1".into())));
    }

    #[test]
    fn an_amendment_that_cannot_go_leaves_the_order_as_it_was() {
        let (mut engine, calls) = engine();
        let id = resting(&mut engine, "2");
        let unacked = engine.submit(buy("1")).id;
        let market = engine
            .submit(NewOrder::market(
                InstrumentId::new("sim", "AAA"),
                Side::Buy,
                d("1"),
            ))
            .id;

        assert_eq!(
            engine.amend(id, d("11"), d("5")),
            Err(AmendError::Denied(RiskReject::QuantityTooLarge {
                max: d("10")
            }))
        );
        assert_eq!(
            engine.amend(unacked, d("2"), d("5")),
            Err(AmendError::NotAmendable(OrderStatus::Submitted))
        );
        assert_eq!(
            engine.amend(market, d("2"), d("5")),
            Err(AmendError::NotAmendable(OrderStatus::Submitted))
        );
        assert_eq!(
            engine.amend(ClientOrderId(99), d("2"), d("5")),
            Err(AmendError::UnknownOrder(ClientOrderId(99)))
        );
        assert_eq!(
            engine.order(id).map(|o| o.status),
            Some(OrderStatus::Accepted)
        );
        assert!(calls.lock().unwrap().amended.is_empty());
    }

    #[test]
    fn an_amendment_needs_a_limit_order_above_its_fills_and_a_venue_that_amends() {
        let (mut engine, _) = engine();
        let id = resting(&mut engine, "4");
        engine
            .apply(&report(
                id,
                OrderEvent::Filled {
                    fill_id: FillId("F1".into()),
                    quantity: d("3"),
                    price: d("5"),
                },
            ))
            .unwrap();

        assert_eq!(
            engine.amend(id, d("3"), d("5")),
            Err(AmendError::BelowFilled(d("3")))
        );

        let calls = Arc::new(Mutex::new(Calls::default()));
        let client = RecordingClient {
            calls,
            refuse_with: None,
            amends: false,
        };
        let limits = RiskLimits {
            max_order_quantity: d("10"),
            max_order_notional: d("1000"),
        };
        let mut plain = ExecutionEngine::new(Box::new(client), limits);
        let other = resting(&mut plain, "2");
        assert_eq!(
            plain.amend(other, d("3"), d("5")),
            Err(AmendError::Unsupported)
        );
        assert!(engine.can_amend());
        assert!(!plain.can_amend());
    }

    #[test]
    fn only_a_resting_limit_order_is_amendable() {
        let (mut engine, _) = engine();
        let id = resting(&mut engine, "2");
        let unacked = engine.submit(buy("1")).id;
        let market = engine
            .submit(NewOrder::market(
                InstrumentId::new("sim", "AAA"),
                Side::Buy,
                d("1"),
            ))
            .id;
        engine
            .apply(&report(
                market,
                OrderEvent::Accepted {
                    venue_order_id: VenueOrderId("V2".into()),
                },
            ))
            .unwrap();
        let amendable = |engine: &ExecutionEngine, id| engine.order(id).is_some_and(is_amendable);

        assert!(amendable(&engine, id));
        assert!(!amendable(&engine, unacked));
        assert!(!amendable(&engine, market));
        assert_eq!(
            engine.amend(market, d("2"), d("5")),
            Err(AmendError::NotALimitOrder)
        );

        engine.amend(id, d("3"), d("5")).unwrap();
        assert!(!amendable(&engine, id), "an amendment is in flight");
    }

    #[test]
    fn an_amended_sell_counts_its_own_shares_once() {
        let (mut engine, calls) = engine();
        let instrument = InstrumentId::new("sim", "AAA");
        let ticks = model::TickTable::flat(d("0.01")).unwrap();
        engine.set_instrument(
            InstrumentInfo::new(instrument.clone(), Decimal::ONE, ticks).with_short_selling(false),
        );
        let held = model::Position::new(instrument.clone(), d("6"), d("5"));
        engine.set_account(AccountState::new(d("1000"), vec![held]));
        let id = engine
            .submit(NewOrder::limit(instrument, Side::Sell, d("4"), d("5")))
            .id;
        engine
            .apply(&report(
                id,
                OrderEvent::Accepted {
                    venue_order_id: VenueOrderId("V1".into()),
                },
            ))
            .unwrap();

        // Six held, this sell's own four among them: 7 is too many, 6 is not.
        assert!(matches!(
            engine.amend(id, d("7"), d("5")),
            Err(AmendError::Denied(RiskReject::NotEnoughToSell { .. }))
        ));
        assert!(engine.amend(id, d("6"), d("5")).is_ok());
        assert_eq!(calls.lock().unwrap().amended.len(), 1);
    }

    #[test]
    fn a_sell_amended_up_counts_at_its_new_size_until_the_venue_answers() {
        let (mut engine, _) = engine();
        let instrument = InstrumentId::new("sim", "AAA");
        let ticks = model::TickTable::flat(d("0.01")).unwrap();
        engine.set_instrument(
            InstrumentInfo::new(instrument.clone(), Decimal::ONE, ticks).with_short_selling(false),
        );
        let held = model::Position::new(instrument.clone(), d("8"), d("5"));
        engine.set_account(AccountState::new(d("1000"), vec![held]));
        let sell =
            |quantity: &str| NewOrder::limit(instrument.clone(), Side::Sell, d(quantity), d("5"));
        let first = engine.submit(sell("4")).id;
        engine
            .apply(&report(
                first,
                OrderEvent::Accepted {
                    venue_order_id: VenueOrderId("V1".into()),
                },
            ))
            .unwrap();
        engine.amend(first, d("8"), d("5")).unwrap();

        // Eight held, eight asked for by the amendment in flight: none left.
        let second = engine.submit(sell("4"));
        engine
            .apply(&report(
                first,
                OrderEvent::AmendRejected {
                    reason: "no".into(),
                },
            ))
            .unwrap();
        let third = engine.submit(sell("4"));

        assert_eq!(second.status, OrderStatus::Denied);
        assert_eq!(third.status, OrderStatus::Submitted);
    }

    fn snapshot(id: ClientOrderId, venue: VenueOrder) -> ExecutionReport {
        report(id, OrderEvent::Snapshot(venue))
    }

    #[test]
    fn missed_fills_are_caught_up_as_one_at_the_price_that_keeps_the_average() {
        let (mut engine, _) = engine();
        let id = resting(&mut engine, "10");
        engine
            .apply(&report(
                id,
                OrderEvent::Filled {
                    fill_id: FillId("F1".into()),
                    quantity: d("2"),
                    price: d("5"),
                },
            ))
            .unwrap();
        // The venue filled 6 in all at an average of 5.5; core saw 2 at 5.
        let view = VenueOrder::new(VenueOrderState::Working, d("6"), Some(d("5.5")));

        let caught_up = engine.apply(&snapshot(id, view.clone())).unwrap();
        let again = engine.apply(&snapshot(id, view)).unwrap();

        assert_eq!(caught_up.filled_quantity, d("6"));
        assert_eq!(caught_up.avg_fill_price, Some(d("5.5")));
        assert_eq!(caught_up.status, OrderStatus::PartiallyFilled);
        assert_eq!(again, caught_up);
    }

    #[test]
    fn a_missed_ack_and_a_missed_end_are_caught_up() {
        let (mut engine, _) = engine();
        let unacked = engine.submit(buy("2")).id;
        let cancelled = resting(&mut engine, "2");
        let rejected = engine.submit(buy("2")).id;

        let acked = engine
            .apply(&snapshot(
                unacked,
                VenueOrder::new(VenueOrderState::Working, Decimal::ZERO, None)
                    .with_venue_order_id(VenueOrderId("V9".into())),
            ))
            .unwrap();
        let ended = engine
            .apply(&snapshot(
                cancelled,
                VenueOrder::new(VenueOrderState::Cancelled, Decimal::ZERO, None),
            ))
            .unwrap();
        let refused = engine
            .apply(&snapshot(
                rejected,
                VenueOrder::new(
                    VenueOrderState::Rejected {
                        reason: "halted".into(),
                    },
                    Decimal::ZERO,
                    None,
                ),
            ))
            .unwrap();

        assert_eq!(acked.status, OrderStatus::Accepted);
        assert_eq!(acked.venue_order_id, Some(VenueOrderId("V9".into())));
        assert_eq!(ended.status, OrderStatus::Cancelled);
        assert_eq!(refused.status, OrderStatus::Rejected);
        assert_eq!(refused.reason.as_deref(), Some("halted"));
    }

    #[test]
    fn a_filled_order_the_stream_missed_ends_filled() {
        let (mut engine, _) = engine();
        let id = resting(&mut engine, "4");

        let order = engine
            .apply(&snapshot(
                id,
                VenueOrder::new(VenueOrderState::Filled, d("4"), Some(d("5"))),
            ))
            .unwrap();

        assert_eq!(order.status, OrderStatus::Filled);
        assert_eq!(order.avg_fill_price, Some(d("5")));
    }

    #[test]
    fn a_rounded_venue_average_still_catches_up_at_the_venues_price() {
        let (mut engine, _) = engine();
        let id = resting(&mut engine, "10");
        engine
            .apply(&report(
                id,
                OrderEvent::Filled {
                    fill_id: FillId("F1".into()),
                    quantity: d("3"),
                    price: d("5.0133"),
                },
            ))
            .unwrap();

        // 4 filled at a rounded 5.01: the exact price would be below zero.
        let order = engine
            .apply(&snapshot(
                id,
                VenueOrder::new(VenueOrderState::Cancelled, d("4"), Some(d("5.01"))),
            ))
            .unwrap();

        assert_eq!(order.filled_quantity, d("4"));
        assert_eq!(order.status, OrderStatus::Cancelled);
    }

    #[test]
    fn missed_fills_without_a_price_are_not_invented_but_the_end_still_applies() {
        let (mut engine, _) = engine();
        let id = engine.submit(buy("4")).id;

        let order = engine
            .apply(&snapshot(
                id,
                VenueOrder::new(VenueOrderState::Cancelled, d("2"), None)
                    .with_venue_order_id(VenueOrderId("V1".into())),
            ))
            .unwrap();
        let unknown = engine.apply(&snapshot(
            ClientOrderId(404),
            VenueOrder::new(VenueOrderState::Working, Decimal::ZERO, None),
        ));

        assert_eq!(order.status, OrderStatus::Cancelled);
        assert_eq!(order.venue_order_id, Some(VenueOrderId("V1".into())));
        assert_eq!(order.filled_quantity, Decimal::ZERO);
        assert!(order.reason.unwrap().contains("2 more filled"));
        assert_eq!(unknown, Err(ApplyError::UnknownOrder(ClientOrderId(404))));
    }

    #[test]
    fn the_venues_terms_settle_an_amendment_whose_answer_was_missed() {
        let (mut engine, _) = engine();
        let id = resting(&mut engine, "4");
        engine.amend(id, d("6"), d("5.5")).unwrap();

        let order = engine
            .apply(&snapshot(
                id,
                VenueOrder::new(VenueOrderState::Working, Decimal::ZERO, None)
                    .with_terms(d("6"), Some(d("5.5"))),
            ))
            .unwrap();

        assert_eq!(order.status, OrderStatus::Accepted);
        assert_eq!(
            (order.quantity, order.limit_price),
            (d("6"), Some(d("5.5")))
        );
    }

    #[test]
    fn filled_on_the_venue_at_fewer_than_core_thought_ends_filled() {
        let (mut engine, _) = engine();
        let id = resting(&mut engine, "6");

        // An amendment down to 4 was missed; the venue filled all 4.
        let order = engine
            .apply(&snapshot(
                id,
                VenueOrder::new(VenueOrderState::Filled, d("4"), Some(d("5"))),
            ))
            .unwrap();

        assert_eq!(order.status, OrderStatus::Filled);
        assert_eq!((order.quantity, order.filled_quantity), (d("4"), d("4")));
    }

    fn sell(quantity: &str) -> NewOrder {
        NewOrder::limit(
            InstrumentId::new("sim", "AAA"),
            Side::Sell,
            d(quantity),
            d("5"),
        )
    }

    /// An engine holding `held` shares of AAA (negative: short).
    fn holding(held: &str) -> (ExecutionEngine, Arc<Mutex<Calls>>) {
        let (mut engine, calls) = engine();
        let position = model::Position::new(InstrumentId::new("sim", "AAA"), d(held), d("5"));
        engine.set_account(AccountState::new(d("1000"), vec![position]));
        (engine, calls)
    }

    #[test]
    fn a_halted_terminal_denies_new_orders_and_still_cancels() {
        let (mut engine, calls) = engine();
        let working = engine.submit(buy("1")).id;

        assert!(engine.set_trading_state(TradingState::Halted));
        let denied = engine.submit(buy("1"));

        assert_eq!(denied.status, OrderStatus::Denied);
        assert_eq!(
            denied.reason.as_deref(),
            Some(RiskReject::TradingHalted.to_string().as_str())
        );
        assert_eq!(calls.lock().unwrap().submitted.len(), 1);
        assert!(engine.cancel(working).is_ok());
    }

    #[test]
    fn a_halted_terminal_denies_amendments() {
        let (mut engine, calls) = engine();
        let id = engine.submit(buy("1")).id;
        engine
            .apply(&report(
                id,
                OrderEvent::Accepted {
                    venue_order_id: VenueOrderId("V1".into()),
                },
            ))
            .unwrap();
        engine.set_trading_state(TradingState::Halted);

        assert_eq!(
            engine.amend(id, d("2"), d("5")),
            Err(AmendError::Denied(RiskReject::TradingHalted))
        );
        assert!(calls.lock().unwrap().amended.is_empty());
    }

    #[test]
    fn reducing_takes_only_what_closes_a_position() {
        let (mut engine, _) = holding("6");
        engine.set_trading_state(TradingState::Reducing);

        assert_eq!(engine.submit(buy("1")).status, OrderStatus::Denied);
        assert_eq!(engine.submit(sell("7")).status, OrderStatus::Denied);
        assert_eq!(engine.submit(sell("4")).status, OrderStatus::Submitted);
        // Four of the six are already in a working sell.
        assert_eq!(
            engine.submit(sell("3")).reason,
            Some(RiskReject::ReducingOnly { closable: d("2") }.to_string())
        );
        assert_eq!(engine.submit(sell("2")).status, OrderStatus::Submitted);
    }

    #[test]
    fn reducing_lets_a_buy_cover_a_short_and_no_more() {
        let (mut engine, _) = holding("-3");
        engine.set_trading_state(TradingState::Reducing);

        assert_eq!(engine.submit(sell("1")).status, OrderStatus::Denied);
        assert_eq!(engine.submit(buy("4")).status, OrderStatus::Denied);
        assert_eq!(engine.submit(buy("3")).status, OrderStatus::Submitted);
    }

    #[test]
    fn reducing_still_lets_a_working_order_shrink_but_not_grow() {
        let (mut engine, _) = engine();
        let accepted = |engine: &mut ExecutionEngine, venue: &str| {
            let id = engine.submit(buy("4")).id;
            engine
                .apply(&report(
                    id,
                    OrderEvent::Accepted {
                        venue_order_id: VenueOrderId(venue.into()),
                    },
                ))
                .unwrap();
            id
        };
        let shrinking = accepted(&mut engine, "V1");
        let growing = accepted(&mut engine, "V2");
        engine.set_trading_state(TradingState::Reducing);

        assert!(engine.amend(shrinking, d("2"), d("5")).is_ok());
        assert!(matches!(
            engine.amend(growing, d("5"), d("5")),
            Err(AmendError::Denied(RiskReject::ReducingOnly { .. }))
        ));
    }

    #[test]
    fn the_day_counts_this_sessions_fills_against_the_venues_position() {
        let (mut engine, _) = engine();
        let instrument = InstrumentId::new("sim", "AAA");
        let id = engine.submit(buy("4")).id;
        engine.apply(&report(id, fill("F1", "4"))).unwrap();
        engine.set_account(AccountState::new(
            d("980"),
            vec![model::Position::new(instrument.clone(), d("4"), d("5"))],
        ));

        let day = engine.day_pnl(|_| Some(d("6")));

        // Four bought at 5 today, worth 6 each now.
        assert_eq!(day.len(), 1);
        assert_eq!(day[0].instrument, instrument);
        assert_eq!(day[0].pnl, Some(d("4")));
    }

    #[test]
    fn active_again_takes_orders_as_before() {
        let (mut engine, _) = engine();
        engine.set_trading_state(TradingState::Halted);

        assert!(!engine.set_trading_state(TradingState::Halted));
        assert!(engine.set_trading_state(TradingState::Active));
        assert_eq!(engine.trading_state(), TradingState::Active);
        assert_eq!(engine.submit(buy("1")).status, OrderStatus::Submitted);
    }
}
