//! Serializable views of core types. Core stays free of serde; the shell owns
//! the wire shape the front end sees.
//!
//! Prices, quantities, and cash cross as decimal strings, never JSON numbers:
//! a JS number would round them, and the front end only needs them for
//! display. Timestamps cross as milliseconds, which a JS number holds exactly.

use data_engine::{Bar, DataEngine};
use model::{
    AccountState, BookLevel, Decimal, InstrumentId, InstrumentInfo, Order, OrderStatus, Quote, Side,
};
use serde::Serialize;

#[derive(Serialize)]
pub struct Snapshot {
    /// The last batch number taken before this snapshot read engine state.
    /// Batches numbered up to it are already reflected here.
    pub seq: u64,
    pub market: MarketDto,
    pub instruments: Vec<InstrumentDto>,
    pub boards: Vec<BoardDto>,
    pub quotes: Vec<QuoteDto>,
    pub orders: Vec<OrderDto>,
    pub account: AccountDto,
}

/// Which market the terminal is wired to, so the front end can pick
/// defaults (layout, price units) without knowing any market by name.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct MarketDto {
    pub id: String,
    pub title: String,
    pub currency: String,
}

/// How an instrument trades: what an order ticket needs to offer only valid
/// quantities and prices.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct InstrumentDto {
    pub symbol: String,
    pub name: String,
    pub exchange: String,
    pub currency: String,
    pub lot_size: String,
    /// Ascending; the tick at a price is that of the last step at or below it.
    pub ticks: Vec<TickStepDto>,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct TickStepDto {
    pub from: String,
    pub tick: String,
}

impl From<&InstrumentInfo> for InstrumentDto {
    fn from(info: &InstrumentInfo) -> Self {
        Self {
            symbol: info.id.symbol.clone(),
            name: info.name.clone(),
            exchange: info.exchange.clone(),
            currency: info.currency.clone(),
            lot_size: number(info.lot_size),
            ticks: info
                .ticks
                .steps()
                .iter()
                .map(|(from, tick)| TickStepDto {
                    from: number(*from),
                    tick: number(*tick),
                })
                .collect(),
        }
    }
}

/// One row of a price board: everything known about an instrument today,
/// in one piece so a row never shows a ceiling from one moment and a last
/// price from another.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct BoardDto {
    pub symbol: String,
    pub exchange: String,
    pub reference: Option<String>,
    pub ceiling: Option<String>,
    pub floor: Option<String>,
    /// Best first.
    pub bids: Vec<LevelDto>,
    /// Best first.
    pub asks: Vec<LevelDto>,
    pub last: Option<String>,
    pub last_quantity: Option<String>,
    /// Last minus reference.
    pub change: Option<String>,
    /// Change as a percentage of reference, to two places.
    pub change_pct: Option<String>,
    pub volume: String,
    pub high: Option<String>,
    pub low: Option<String>,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct LevelDto {
    pub price: String,
    pub size: String,
}

impl From<&BookLevel> for LevelDto {
    fn from(level: &BookLevel) -> Self {
        Self {
            price: number(level.price),
            size: number(level.size),
        }
    }
}

const PERCENT_PLACES: u32 = 2;

impl BoardDto {
    /// The row as the data engine knows it now. Depth when the venue sends
    /// it, else the top of book from the latest quote.
    pub fn of(data: &DataEngine, instrument: &InstrumentId) -> Self {
        let band = data.band(instrument);
        let stats = data.stats(instrument).cloned().unwrap_or_default();
        let (bids, asks) = match (data.depth(instrument), data.latest(instrument)) {
            (Some(depth), _) => (levels(&depth.bids), levels(&depth.asks)),
            (None, Some(quote)) => (
                vec![level(quote.bid, quote.bid_size)],
                vec![level(quote.ask, quote.ask_size)],
            ),
            (None, None) => (vec![], vec![]),
        };
        let reference = band.map(|b| b.reference);
        let change = stats
            .last
            .zip(reference)
            .and_then(|(last, r)| last.checked_sub(r));
        let change_pct = change.zip(reference).and_then(|(change, r)| {
            change
                .checked_mul(Decimal::ONE_HUNDRED)
                .and_then(|scaled| scaled.checked_div(r))
                .map(|pct| pct.round_dp(PERCENT_PLACES))
        });
        Self {
            symbol: instrument.symbol.clone(),
            exchange: data
                .instrument(instrument)
                .map(|info| info.exchange.clone())
                .unwrap_or_default(),
            reference: reference.map(number),
            ceiling: band.and_then(|b| b.ceiling).map(number),
            floor: band.and_then(|b| b.floor).map(number),
            bids,
            asks,
            last: stats.last.map(number),
            last_quantity: stats.last_quantity.map(number),
            change: change.map(number),
            change_pct: change_pct.map(number),
            volume: number(stats.volume),
            high: stats.high.map(number),
            low: stats.low.map(number),
        }
    }
}

fn levels(book: &[BookLevel]) -> Vec<LevelDto> {
    book.iter().map(LevelDto::from).collect()
}

fn level(price: Decimal, size: Decimal) -> LevelDto {
    LevelDto {
        price: number(price),
        size: number(size),
    }
}

/// Without trailing zeros that tick arithmetic leaves behind: a ceiling
/// computed as 62500 * 1.07, rounded to the tick, is 66800, not 66800.00.
fn number(value: Decimal) -> String {
    value.normalize().to_string()
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
