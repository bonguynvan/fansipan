//! Serializable views of core types. Core stays free of serde; the shell owns
//! the wire shape the front end sees.
//!
//! Prices, quantities, and cash cross as decimal strings, never JSON numbers:
//! a JS number would round them, and the front end only needs them for
//! display. Timestamps cross as milliseconds, which a JS number holds exactly.

use data_engine::Bar;
use model::{AccountState, Order, OrderStatus, Quote, Side};
use serde::Serialize;

#[derive(Serialize)]
pub struct Snapshot {
    /// The last batch number taken before this snapshot read engine state.
    /// Batches numbered up to it are already reflected here.
    pub seq: u64,
    pub quotes: Vec<QuoteDto>,
    pub orders: Vec<OrderDto>,
    pub account: AccountDto,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct QuoteDto {
    pub symbol: String,
    pub bid: String,
    pub ask: String,
    pub bid_size: String,
    pub ask_size: String,
    pub ts_ms: u64,
}

const NANOS_PER_MILLI: u64 = 1_000_000;

impl From<&Quote> for QuoteDto {
    fn from(quote: &Quote) -> Self {
        Self {
            symbol: quote.instrument.symbol.clone(),
            bid: quote.bid.to_string(),
            ask: quote.ask.to_string(),
            bid_size: quote.bid_size.to_string(),
            ask_size: quote.ask_size.to_string(),
            ts_ms: quote.ts_event.0 / NANOS_PER_MILLI,
        }
    }
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct BarDto {
    pub symbol: String,
    /// Start of the bar's interval.
    pub time_ms: u64,
    pub open: String,
    pub high: String,
    pub low: String,
    pub close: String,
    /// Quotes in the bar: tick volume, since a quote feed has no traded volume.
    pub ticks: u64,
}

impl BarDto {
    pub fn new(symbol: &str, bar: &Bar) -> Self {
        Self {
            symbol: symbol.to_string(),
            time_ms: bar.ts_open.0 / NANOS_PER_MILLI,
            open: bar.open.to_string(),
            high: bar.high.to_string(),
            low: bar.low.to_string(),
            close: bar.close.to_string(),
            ticks: bar.ticks,
        }
    }
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct OrderDto {
    pub id: u64,
    pub venue_order_id: Option<String>,
    pub symbol: String,
    pub side: &'static str,
    pub quantity: String,
    pub filled_quantity: String,
    pub limit_price: Option<String>,
    pub avg_fill_price: Option<String>,
    pub status: &'static str,
    pub reason: Option<String>,
    /// Whether a cancel makes sense, so the front end does not re-derive the
    /// engine's notion of "open".
    pub open: bool,
}

impl From<&Order> for OrderDto {
    fn from(order: &Order) -> Self {
        Self {
            id: order.id.0,
            venue_order_id: order.venue_order_id.as_ref().map(|v| v.0.clone()),
            symbol: order.instrument.symbol.clone(),
            side: match order.side {
                Side::Buy => "buy",
                Side::Sell => "sell",
            },
            quantity: order.quantity.to_string(),
            filled_quantity: order.filled_quantity.to_string(),
            limit_price: order.limit_price.map(|p| p.to_string()),
            avg_fill_price: order.avg_fill_price.map(|p| p.to_string()),
            status: status_label(order.status),
            reason: order.reason.clone(),
            open: order.status.is_open(),
        }
    }
}

fn status_label(status: OrderStatus) -> &'static str {
    match status {
        OrderStatus::Submitted => "submitted",
        OrderStatus::Accepted => "accepted",
        OrderStatus::PartiallyFilled => "partial",
        OrderStatus::Filled => "filled",
        OrderStatus::PendingCancel => "cancelling",
        OrderStatus::Cancelled => "cancelled",
        OrderStatus::Rejected => "rejected",
        OrderStatus::Denied => "denied",
        _ => "unknown",
    }
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct AccountDto {
    pub cash: String,
    pub positions: Vec<PositionDto>,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct PositionDto {
    pub symbol: String,
    pub quantity: String,
    pub avg_price: String,
}

impl From<&AccountState> for AccountDto {
    fn from(account: &AccountState) -> Self {
        Self {
            cash: account.cash.to_string(),
            positions: account
                .positions
                .iter()
                .map(|p| PositionDto {
                    symbol: p.instrument.symbol.clone(),
                    quantity: p.quantity.to_string(),
                    avg_price: p.avg_price.to_string(),
                })
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use model::{ClientOrderId, Decimal, InstrumentId, UnixNanos};

    #[test]
    fn quote_prices_cross_as_exact_strings_and_time_as_millis() {
        let quote = Quote {
            instrument: InstrumentId::new("sim", "AAA"),
            bid: "0.1".parse().unwrap(),
            ask: "0.3".parse().unwrap(),
            bid_size: Decimal::ONE,
            ask_size: Decimal::ONE,
            ts_event: UnixNanos(1_700_000_000_123_456_789),
        };

        let dto = QuoteDto::from(&quote);

        assert_eq!(dto.bid, "0.1");
        assert_eq!(dto.ask, "0.3");
        assert_eq!(dto.ts_ms, 1_700_000_000_123);
    }

    #[test]
    fn order_carries_status_label_and_cancellability() {
        let order = Order {
            id: ClientOrderId(7),
            venue_order_id: None,
            instrument: InstrumentId::new("sim", "AAA"),
            side: Side::Sell,
            quantity: Decimal::TEN,
            limit_price: None,
            status: OrderStatus::PendingCancel,
            filled_quantity: Decimal::ZERO,
            avg_fill_price: None,
            reason: None,
        };

        let dto = OrderDto::from(&order);

        assert_eq!(dto.status, "cancelling");
        assert_eq!(dto.side, "sell");
        assert!(dto.open);
    }
}
