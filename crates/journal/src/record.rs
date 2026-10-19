//! One journal line: an order, a conditional order, or the trader's switch,
//! as text that reads back to exactly what was written. Decimals are
//! written as their exact text, never as floating point. A value this
//! version does not know is refused, not guessed at.

use std::fmt;

use execution_engine::{
    Conditional, ConditionalId, ConditionalStatus, Offset, OrderRecord, Then, TradingState, Trigger,
};
use model::{
    ClientOrderId, Decimal, FillId, InstrumentId, Order, OrderKind, OrderStatus, OrderType, Side,
    TimeInForce, VenueOrderId,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordError(pub String);

impl fmt::Display for RecordError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for RecordError {}

fn unknown(what: &str, value: impl fmt::Debug) -> RecordError {
    RecordError(format!("this version cannot record {what} {value:?}"))
}

fn decimal(field: &str, raw: &str) -> Result<Decimal, RecordError> {
    raw.parse()
        .map_err(|_| RecordError(format!("{field} is not a decimal: {raw:?}")))
}

fn optional(field: &str, raw: &Option<String>) -> Result<Option<Decimal>, RecordError> {
    raw.as_deref().map(|raw| decimal(field, raw)).transpose()
}

/// A journal line.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Record {
    Order(OrderLine),
    Conditional(ConditionalLine),
    Trading(String),
}

impl Record {
    pub fn order(record: &OrderRecord) -> Result<Self, RecordError> {
        OrderLine::from_record(record).map(Self::Order)
    }

    pub fn conditional(conditional: &Conditional) -> Result<Self, RecordError> {
        ConditionalLine::from_conditional(conditional).map(Self::Conditional)
    }

    pub fn trading(state: TradingState) -> Self {
        Self::Trading(trading_label(state).to_string())
    }
}

pub(crate) fn trading_label(state: TradingState) -> &'static str {
    match state {
        TradingState::Active => "active",
        TradingState::Reducing => "reducing",
        TradingState::Halted => "halted",
    }
}

pub(crate) fn parse_trading(raw: &str) -> Result<TradingState, RecordError> {
    match raw {
        "active" => Ok(TradingState::Active),
        "reducing" => Ok(TradingState::Reducing),
        "halted" => Ok(TradingState::Halted),
        other => Err(unknown("the trading state", other)),
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OrderLine {
    pub id: u64,
    pub venue_order_id: Option<String>,
    pub venue: String,
    pub symbol: String,
    pub side: String,
    pub quantity: String,
    pub kind: String,
    pub time_in_force: String,
    pub limit_price: Option<String>,
    pub status: String,
    pub filled_quantity: String,
    pub avg_fill_price: Option<String>,
    pub reason: Option<String>,
    pub applied_fills: Vec<String>,
    pub amending_to: Option<String>,
}

fn side_label(side: Side) -> &'static str {
    match side {
        Side::Buy => "buy",
        Side::Sell => "sell",
    }
}

fn parse_side(raw: &str) -> Result<Side, RecordError> {
    match raw {
        "buy" => Ok(Side::Buy),
        "sell" => Ok(Side::Sell),
        other => Err(unknown("the side", other)),
    }
}

fn kind_label(kind: OrderKind) -> Result<&'static str, RecordError> {
    match kind {
        OrderKind::Limit => Ok("limit"),
        OrderKind::Market => Ok("market"),
        OrderKind::MarketToLimit => Ok("market_to_limit"),
        other => Err(unknown("the order kind", other)),
    }
}

fn parse_kind(raw: &str) -> Result<OrderKind, RecordError> {
    match raw {
        "limit" => Ok(OrderKind::Limit),
        "market" => Ok(OrderKind::Market),
        "market_to_limit" => Ok(OrderKind::MarketToLimit),
        other => Err(unknown("the order kind", other)),
    }
}

fn time_in_force_label(tif: TimeInForce) -> Result<&'static str, RecordError> {
    match tif {
        TimeInForce::Day => Ok("day"),
        TimeInForce::ImmediateOrCancel => Ok("ioc"),
        TimeInForce::FillOrKill => Ok("fok"),
        TimeInForce::AtOpen => Ok("at_open"),
        TimeInForce::AtClose => Ok("at_close"),
        other => Err(unknown("the time in force", other)),
    }
}

fn parse_time_in_force(raw: &str) -> Result<TimeInForce, RecordError> {
    match raw {
        "day" => Ok(TimeInForce::Day),
        "ioc" => Ok(TimeInForce::ImmediateOrCancel),
        "fok" => Ok(TimeInForce::FillOrKill),
        "at_open" => Ok(TimeInForce::AtOpen),
        "at_close" => Ok(TimeInForce::AtClose),
        other => Err(unknown("the time in force", other)),
    }
}

fn status_label(status: OrderStatus) -> Result<&'static str, RecordError> {
    match status {
        OrderStatus::Submitted => Ok("submitted"),
        OrderStatus::Accepted => Ok("accepted"),
        OrderStatus::PartiallyFilled => Ok("partially_filled"),
        OrderStatus::Filled => Ok("filled"),
        OrderStatus::PendingCancel => Ok("pending_cancel"),
        OrderStatus::PendingAmend => Ok("pending_amend"),
        OrderStatus::Cancelled => Ok("cancelled"),
        OrderStatus::Rejected => Ok("rejected"),
        OrderStatus::Denied => Ok("denied"),
        other => Err(unknown("the order status", other)),
    }
}

fn parse_status(raw: &str) -> Result<OrderStatus, RecordError> {
    match raw {
        "submitted" => Ok(OrderStatus::Submitted),
        "accepted" => Ok(OrderStatus::Accepted),
        "partially_filled" => Ok(OrderStatus::PartiallyFilled),
        "filled" => Ok(OrderStatus::Filled),
        "pending_cancel" => Ok(OrderStatus::PendingCancel),
        "pending_amend" => Ok(OrderStatus::PendingAmend),
        "cancelled" => Ok(OrderStatus::Cancelled),
        "rejected" => Ok(OrderStatus::Rejected),
        "denied" => Ok(OrderStatus::Denied),
        other => Err(unknown("the order status", other)),
    }
}

impl OrderLine {
    fn from_record(record: &OrderRecord) -> Result<Self, RecordError> {
        let order = &record.order;
        Ok(Self {
            id: order.id.0,
            venue_order_id: order.venue_order_id.as_ref().map(|id| id.0.clone()),
            venue: order.instrument.venue.clone(),
            symbol: order.instrument.symbol.clone(),
            side: side_label(order.side).into(),
            quantity: order.quantity.to_string(),
            kind: kind_label(order.order_type.kind)?.into(),
            time_in_force: time_in_force_label(order.order_type.time_in_force)?.into(),
            limit_price: order.limit_price.map(|p| p.to_string()),
            status: status_label(order.status)?.into(),
            filled_quantity: order.filled_quantity.to_string(),
            avg_fill_price: order.avg_fill_price.map(|p| p.to_string()),
            reason: order.reason.clone(),
            applied_fills: record.applied_fills.iter().map(|f| f.0.clone()).collect(),
            amending_to: record.amending_to.map(|q| q.to_string()),
        })
    }

    pub fn to_record(&self) -> Result<OrderRecord, RecordError> {
        Ok(OrderRecord {
            order: Order {
                id: ClientOrderId(self.id),
                venue_order_id: self.venue_order_id.clone().map(VenueOrderId),
                instrument: InstrumentId::new(self.venue.as_str(), self.symbol.as_str()),
                side: parse_side(&self.side)?,
                quantity: decimal("quantity", &self.quantity)?,
                order_type: OrderType::new(
                    parse_kind(&self.kind)?,
                    parse_time_in_force(&self.time_in_force)?,
                ),
                limit_price: optional("limit price", &self.limit_price)?,
                status: parse_status(&self.status)?,
                filled_quantity: decimal("filled quantity", &self.filled_quantity)?,
                avg_fill_price: optional("average fill price", &self.avg_fill_price)?,
                reason: self.reason.clone(),
            },
            applied_fills: self.applied_fills.iter().cloned().map(FillId).collect(),
            amending_to: optional("amendment quantity", &self.amending_to)?,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConditionalLine {
    pub id: u64,
    pub venue: String,
    pub symbol: String,
    pub side: String,
    pub quantity: String,
    /// "at_or_below", "at_or_above", "trailing_amount", or "trailing_percent".
    pub trigger: String,
    /// The trigger's price, or the trailing distance.
    pub trigger_value: String,
    /// The limit price it sends at, or `None` for a marketable order.
    pub limit_price: Option<String>,
    /// "pending", "waiting", "fired", or "cancelled".
    pub status: String,
    pub fired_order: Option<u64>,
    pub best: Option<String>,
    pub level: Option<String>,
    pub blocked: Option<String>,
    pub after: Option<u64>,
    pub oco: Option<u64>,
    pub problem: Option<String>,
}

impl ConditionalLine {
    fn from_conditional(c: &Conditional) -> Result<Self, RecordError> {
        let (trigger, value) = match c.trigger {
            Trigger::AtOrBelow(price) => ("at_or_below", price),
            Trigger::AtOrAbove(price) => ("at_or_above", price),
            Trigger::Trailing(Offset::Amount(amount)) => ("trailing_amount", amount),
            Trigger::Trailing(Offset::Percent(percent)) => ("trailing_percent", percent),
        };
        let (status, fired_order) = match c.status {
            ConditionalStatus::Pending => ("pending", None),
            ConditionalStatus::Waiting => ("waiting", None),
            ConditionalStatus::Fired(order) => ("fired", Some(order.0)),
            ConditionalStatus::Cancelled => ("cancelled", None),
        };
        Ok(Self {
            id: c.id.0,
            venue: c.instrument.venue.clone(),
            symbol: c.instrument.symbol.clone(),
            side: side_label(c.side).into(),
            quantity: c.quantity.to_string(),
            trigger: trigger.into(),
            trigger_value: value.to_string(),
            limit_price: match c.then {
                Then::Limit(price) => Some(price.to_string()),
                Then::Marketable => None,
            },
            status: status.into(),
            fired_order,
            best: c.best.map(|p| p.to_string()),
            level: c.level.map(|p| p.to_string()),
            blocked: c.blocked.clone(),
            after: c.after.map(|id| id.0),
            oco: c.oco.map(|id| id.0),
            problem: c.problem.clone(),
        })
    }

    pub fn to_conditional(&self) -> Result<Conditional, RecordError> {
        let value = decimal("trigger value", &self.trigger_value)?;
        let trigger = match self.trigger.as_str() {
            "at_or_below" => Trigger::AtOrBelow(value),
            "at_or_above" => Trigger::AtOrAbove(value),
            "trailing_amount" => Trigger::Trailing(Offset::Amount(value)),
            "trailing_percent" => Trigger::Trailing(Offset::Percent(value)),
            other => return Err(unknown("the trigger", other)),
        };
        let status = match (self.status.as_str(), self.fired_order) {
            ("pending", _) => ConditionalStatus::Pending,
            ("waiting", _) => ConditionalStatus::Waiting,
            ("fired", Some(order)) => ConditionalStatus::Fired(ClientOrderId(order)),
            ("cancelled", _) => ConditionalStatus::Cancelled,
            (other, _) => return Err(unknown("the conditional status", other)),
        };
        Ok(Conditional {
            id: ConditionalId(self.id),
            instrument: InstrumentId::new(self.venue.as_str(), self.symbol.as_str()),
            side: parse_side(&self.side)?,
            quantity: decimal("quantity", &self.quantity)?,
            trigger,
            then: match optional("limit price", &self.limit_price)? {
                Some(price) => Then::Limit(price),
                None => Then::Marketable,
            },
            status,
            best: optional("best price", &self.best)?,
            level: optional("level", &self.level)?,
            blocked: self.blocked.clone(),
            after: self.after.map(ClientOrderId),
            oco: self.oco.map(ConditionalId),
            problem: self.problem.clone(),
        })
    }
}
