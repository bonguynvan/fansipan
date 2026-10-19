//! The desktop terminal as a library. [`run`] takes a [`Venue`] (one
//! adapter's two clients and what the shell needs to know about them),
//! wires the bus to the engines, and opens the window, with a small command
//! surface for the web front end.
//!
//! The shell never names an adapter. A binary does that: this crate's own
//! wires the sandbox (`src/main.rs`), and an adapter repo wires its adapter
//! the same way in an example, so its library still depends only on
//! `fansipan-adapter-api` (ADR 0008).

mod batch;
mod dto;
#[cfg(test)]
mod latency_report;
#[cfg(test)]
mod no_panic_tests;
mod settings;
mod workspace;

use std::collections::{BTreeSet, HashMap};
use std::mem;
use std::process::ExitCode;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::{Mutex, MutexGuard};
use std::thread;
use std::time::Duration;

use adapter_api::{AdapterError, DataClient, ExecutionClient};
use bus::{Bus, Event};
use data_engine::DataEngine;
use execution_engine::{
    BracketRequest, ConditionalId, ConditionalRequest, ExecutionEngine, Offset, RiskBudget,
    SizeRequest, Then, Trigger,
};
use model::{ClientOrderId, Connection, Decimal, InstrumentId, NewOrder, Side};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_window_state::StateFlags;

pub use execution_engine::RiskLimits;

use batch::{Batch, BatchDto};
use dto::{
    AccountDto, BarDto, BoardDto, BracketDto, CancelAllDto, CancelFailureDto, CapacityDto,
    ConditionalDto, ConnectionDto, DayDto, InstrumentDto, MarketDto, OrderDto, PrintDto, QuoteDto,
    SessionDto, SizingDto, Snapshot,
};

const EVENT_NAME: &str = "bus-batch";
/// How often conflated changes go to the front end: fast enough that an
/// order's fill feels immediate, slow enough that a burst costs one message.
const FLUSH_INTERVAL: Duration = Duration::from_millis(50);
const DEFAULT_BAR_LIMIT: usize = 500;
const DEFAULT_PRINT_LIMIT: usize = 200;

/// How the front end presents the venue. `id` selects its display presets
/// (price scale, session badge, default layout); a venue without a preset
/// gets plain defaults. `title` and `currency` are shown as they are.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Market {
    pub id: String,
    pub title: String,
    /// ISO 4217 code of the account currency, e.g. "VND".
    pub currency: String,
}

impl Market {
    pub fn new(
        id: impl Into<String>,
        title: impl Into<String>,
        currency: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            currency: currency.into(),
        }
    }
}

/// What the terminal runs against: one venue's clients and what the shell
/// needs to know about it. Build one with [`Venue::new`].
#[non_exhaustive]
pub struct Venue {
    /// The venue part of every `InstrumentId` the adapter publishes. The
    /// front end names instruments by symbol alone, and the shell adds this.
    id: String,
    market: Market,
    data: Box<dyn DataClient>,
    execution: Box<dyn ExecutionClient>,
    limits: RiskLimits,
    /// Subscribed to quotes and trades when the terminal starts.
    instruments: Vec<InstrumentId>,
}

impl Venue {
    pub fn new(
        id: impl Into<String>,
        market: Market,
        data: Box<dyn DataClient>,
        execution: Box<dyn ExecutionClient>,
        limits: RiskLimits,
    ) -> Self {
        Self {
            id: id.into(),
            market,
            data,
            execution,
            limits,
            instruments: Vec::new(),
        }
    }

    /// Adds instruments to subscribe to at start. Each must be on this
    /// venue: `run` refuses one that names another.
    pub fn subscribe(self, instruments: impl IntoIterator<Item = InstrumentId>) -> Self {
        let mut all = self.instruments;
        all.extend(instruments);
        Self {
            instruments: all,
            ..self
        }
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn instruments(&self) -> &[InstrumentId] {
        &self.instruments
    }
}

/// Lock order: `exec` before `pending`. Every change to an order is queued
/// in `pending` while `exec` is still held, so the batch receives order
/// states in the order the engine produced them, whichever thread made the
/// change. Nothing takes `pending` and then another lock.
pub struct Shell {
    venue: String,
    market: MarketDto,
    data: Mutex<DataEngine>,
    exec: Mutex<ExecutionEngine>,
    /// Changes not yet sent to the front end.
    pending: Mutex<Batch>,
    /// Number of the last batch taken from `pending`.
    flushed: AtomicU64,
    /// Conditional orders waiting, so a trade takes the execution lock only
    /// when one may fire.
    waiting: AtomicUsize,
}

/// Engines only become poisoned if a bug panicked mid-update; the shell
/// refuses further use rather than act on state that may be half-written.
fn lock<T>(mutex: &Mutex<T>) -> Result<MutexGuard<'_, T>, String> {
    mutex
        .lock()
        .map_err(|_| "terminal state is unavailable after an internal error; restart".to_string())
}

impl Shell {
    fn instrument(&self, symbol: &str) -> InstrumentId {
        InstrumentId::new(self.venue.as_str(), symbol)
    }
}

fn parse_side(raw: &str) -> Result<Side, String> {
    match raw {
        "buy" => Ok(Side::Buy),
        "sell" => Ok(Side::Sell),
        other => Err(format!("unknown side: {other}")),
    }
}

fn parse_decimal(field: &str, raw: &str) -> Result<Decimal, String> {
    raw.trim()
        .parse()
        .map_err(|_| format!("{field}: \"{raw}\" is not a number"))
}

/// The order type as the ticket named it. Without a kind, a price makes a
/// limit order and no price a market one; without a time in force, it is
/// for the day.
fn parse_type(
    kind: Option<&str>,
    time_in_force: Option<&str>,
    priced: bool,
) -> Result<model::OrderType, String> {
    let kind = kind.unwrap_or(if priced { "limit" } else { "market" });
    let time_in_force = time_in_force.unwrap_or("day");
    dto::parse_order_type(kind, time_in_force)
        .ok_or_else(|| format!("unknown order type: {kind} {time_in_force}"))
}

/// What the ticket asked for, parsed exactly.
struct Ticket<'a> {
    symbol: &'a str,
    side: &'a str,
    quantity: &'a str,
    limit_price: Option<&'a str>,
    kind: Option<&'a str>,
    time_in_force: Option<&'a str>,
}

fn new_order(venue: &str, ticket: &Ticket<'_>) -> Result<NewOrder, String> {
    let limit_price = ticket
        .limit_price
        .map(|raw| parse_decimal("limit price", raw))
        .transpose()?;
    Ok(NewOrder::new(
        InstrumentId::new(venue, ticket.symbol),
        parse_side(ticket.side)?,
        parse_decimal("quantity", ticket.quantity)?,
        parse_type(ticket.kind, ticket.time_in_force, limit_price.is_some())?,
        limit_price,
    ))
}

/// Everything the front end needs to start, numbered against the batches.
fn snapshot_of(shell: &Shell) -> Result<Snapshot, String> {
    // Read before any state: every batch numbered up to `seq` was taken
    // from `pending` before the state below was read, so it holds nothing
    // newer than this snapshot and the front end can drop it.
    let seq = shell.flushed.load(Ordering::SeqCst);
    let (instruments, boards, quotes, data_connection, sessions) = {
        let data = lock(&shell.data)?;
        let known: BTreeSet<&InstrumentId> = data
            .instruments()
            .map(|info| &info.id)
            .chain(data.quotes().map(|quote| &quote.instrument))
            .collect();
        (
            data.instruments().map(InstrumentDto::from).collect(),
            known
                .into_iter()
                .map(|id| BoardDto::of(&data, id))
                .collect(),
            data.quotes().map(QuoteDto::from).collect(),
            ConnectionDto::of(Connection::MarketData, data.connection()),
            data.sessions().map(SessionDto::from).collect(),
        )
    };
    let day = day_of(shell)?;
    let (orders, orders_connection, account, can_amend, trading, conditionals) = {
        let mut exec = lock(&shell.exec)?;
        (
            exec.orders().map(OrderDto::from).collect(),
            ConnectionDto::of(Connection::Orders, exec.connection()),
            AccountDto::from(exec.account()),
            exec.can_amend(),
            dto::trading_label(exec.trading_state()),
            exec.conditionals().map(ConditionalDto::from).collect(),
        )
    };
    Ok(Snapshot {
        seq,
        market: shell.market.clone(),
        instruments,
        boards,
        quotes,
        orders,
        account,
        connections: vec![data_connection, orders_connection],
        sessions,
        can_amend,
        trading,
        day,
        conditionals,
    })
}

/// The day's profit and loss now: each instrument held or traded marked
/// at its last trade, or the middle of its quote before any trade. One
/// engine is locked at a time, so the lock order is never in question.
/// What each of `wanted` is worth now: its last trade, else its quote's mid.
/// One with neither is left out.
fn marks_of(
    shell: &Shell,
    wanted: impl IntoIterator<Item = InstrumentId>,
) -> Result<HashMap<InstrumentId, Decimal>, String> {
    let data = lock(&shell.data)?;
    Ok(wanted
        .into_iter()
        .filter_map(|id| {
            let last = data.stats(&id).and_then(|s| s.last);
            let mark = last.or_else(|| data.latest(&id).map(model::Quote::mid))?;
            Some((id, mark))
        })
        .collect())
}

fn day_of(shell: &Shell) -> Result<DayDto, String> {
    let wanted = lock(&shell.exec)?.day_instruments();
    let marks = marks_of(shell, wanted)?;
    let exec = lock(&shell.exec)?;
    Ok(DayDto::of(&exec.day_pnl(|id| marks.get(id).copied())))
}

/// Takes everything pending as the next numbered batch; `None` if nothing
/// changed since the last one.
fn take_batch(shell: &Shell) -> Result<Option<BatchDto>, String> {
    let batch = mem::take(&mut *lock(&shell.pending)?);
    if batch.is_empty() {
        return Ok(None);
    }
    let seq = shell.flushed.fetch_add(1, Ordering::SeqCst) + 1;
    // Every batch moves a price, an order, or the account, any of which
    // moves the day; recounting it is a pass over what is held or traded.
    // A day that cannot be counted must not cost the batch it rides on.
    Ok(Some(BatchDto {
        day: day_of(shell).ok(),
        ..batch.into_dto(seq)
    }))
}

fn submit(shell: &Shell, request: NewOrder) -> Result<OrderDto, String> {
    let mut exec = lock(&shell.exec)?;
    let order = OrderDto::from(&exec.submit(request));
    lock(&shell.pending)?.order(order.clone());
    Ok(order)
}

fn cancel(shell: &Shell, id: u64) -> Result<OrderDto, String> {
    let mut exec = lock(&shell.exec)?;
    let order = exec
        .cancel(ClientOrderId(id))
        .map(|o| OrderDto::from(&o))
        .map_err(|e| e.to_string())?;
    lock(&shell.pending)?.order(order.clone());
    Ok(order)
}

/// What a trader asked for, parsed exactly: `trigger` is "at_or_below",
/// "at_or_above", or "trailing"; a trailing stop takes `offset`, in percent
/// when `percent`; without `limit_price` it sends a marketable order.
struct ConditionalTicket<'a> {
    symbol: &'a str,
    side: &'a str,
    quantity: &'a str,
    trigger: &'a str,
    price: Option<&'a str>,
    offset: Option<&'a str>,
    percent: bool,
    limit_price: Option<&'a str>,
}

fn conditional_request(
    venue: &str,
    ticket: &ConditionalTicket<'_>,
) -> Result<ConditionalRequest, String> {
    let needed = |field: &str, raw: Option<&str>| {
        raw.ok_or_else(|| format!("{field} is missing"))
            .and_then(|raw| parse_decimal(field, raw))
    };
    let trigger = match ticket.trigger {
        "at_or_below" => Trigger::AtOrBelow(needed("trigger price", ticket.price)?),
        "at_or_above" => Trigger::AtOrAbove(needed("trigger price", ticket.price)?),
        "trailing" if ticket.percent => {
            Trigger::Trailing(Offset::Percent(needed("trailing percent", ticket.offset)?))
        }
        "trailing" => Trigger::Trailing(Offset::Amount(needed("trailing amount", ticket.offset)?)),
        other => return Err(format!("unknown trigger: {other}")),
    };
    let then = match ticket.limit_price {
        Some(raw) => Then::Limit(parse_decimal("limit price", raw)?),
        None => Then::Marketable,
    };
    Ok(ConditionalRequest {
        instrument: InstrumentId::new(venue, ticket.symbol),
        side: parse_side(ticket.side)?,
        quantity: parse_decimal("quantity", ticket.quantity)?,
        trigger,
        then,
    })
}

/// Places a conditional order; a trailing stop starts from the last trade.
fn place_conditional_order(
    shell: &Shell,
    ticket: &ConditionalTicket<'_>,
) -> Result<ConditionalDto, String> {
    let request = conditional_request(&shell.venue, ticket)?;
    // Counted before the mark is read, so a trade the mark misses waits on
    // the execution lock for this placement instead of skipping it.
    shell.waiting.fetch_add(1, Ordering::AcqRel);
    let mark =
        lock(&shell.data).map(|data| data.stats(&request.instrument).and_then(|stats| stats.last));
    let mut exec = lock(&shell.exec)?;
    let placed = mark.and_then(|mark| {
        exec.place_conditional(request, mark)
            .map(|c| ConditionalDto::from(&c))
            .map_err(|e| e.to_string())
    });
    shell
        .waiting
        .store(exec.waiting_conditionals(), Ordering::Release);
    let placed = placed?;
    lock(&shell.pending)?.conditional(placed.clone());
    Ok(placed)
}

fn cancel_conditional_order(shell: &Shell, id: u64) -> Result<ConditionalDto, String> {
    let mut exec = lock(&shell.exec)?;
    let cancelled = exec
        .cancel_conditional(ConditionalId(id))
        .map(|c| ConditionalDto::from(&c))
        .map_err(|e| e.to_string())?;
    shell
        .waiting
        .store(exec.waiting_conditionals(), Ordering::Release);
    lock(&shell.pending)?.conditional(cancelled.clone());
    Ok(cancelled)
}

/// Moves trailing stops and fires what a trade in `instrument` at `price`
/// reaches.
fn fire_conditionals(
    shell: &Shell,
    instrument: &InstrumentId,
    price: Decimal,
) -> Result<(), String> {
    let mut exec = lock(&shell.exec)?;
    let (changed, orders) = exec.on_trade(instrument, price);
    shell
        .waiting
        .store(exec.waiting_conditionals(), Ordering::Release);
    if changed.is_empty() && orders.is_empty() {
        return Ok(());
    }
    let mut pending = lock(&shell.pending)?;
    for conditional in &changed {
        pending.conditional(ConditionalDto::from(conditional));
    }
    for order in &orders {
        pending.order(OrderDto::from(order));
    }
    Ok(())
}

/// What a trader asked to size, parsed exactly: `risk` is an amount of the
/// account's currency, or with `percent` a percentage of its equity.
struct SizingTicket<'a> {
    symbol: &'a str,
    side: &'a str,
    kind: Option<&'a str>,
    time_in_force: Option<&'a str>,
    entry: &'a str,
    stop: &'a str,
    risk: &'a str,
    percent: bool,
}

fn sizing_of(shell: &Shell, ticket: &SizingTicket<'_>) -> Result<SizingDto, String> {
    let risk = parse_decimal("risk", ticket.risk)?;
    let request = SizeRequest {
        instrument: shell.instrument(ticket.symbol),
        side: parse_side(ticket.side)?,
        order_type: parse_type(ticket.kind, ticket.time_in_force, true)?,
        entry: parse_decimal("entry price", ticket.entry)?,
        stop: parse_decimal("stop price", ticket.stop)?,
        budget: if ticket.percent {
            RiskBudget::PercentOfEquity(risk)
        } else {
            RiskBudget::Amount(risk)
        },
    };
    // Equity counts every position, so every position needs its price.
    let held: Vec<InstrumentId> = lock(&shell.exec)?
        .account()
        .positions
        .iter()
        .map(|position| position.instrument.clone())
        .collect();
    let marks = marks_of(shell, held)?;
    let exec = lock(&shell.exec)?;
    exec.size_for_risk(&request, |id| marks.get(id).copied())
        .map(|sizing| SizingDto::from(&sizing))
        .map_err(|e| e.to_string())
}

/// The app's configuration and assets. On Windows the window icon is read
/// from the running executable's resources, which only the shell's own
/// binary carries; a program that opens the terminal as a library (an
/// adapter's example, ADR 0008) gets the icon built into this crate instead.
fn context() -> tauri::Context<tauri::Wry> {
    let mut context = tauri::generate_context!();
    if context.default_window_icon().is_none() {
        context.set_default_window_icon(Some(tauri::include_image!("icons/icon.png")));
    }
    context
}

/// What the ticket asked for with exits, parsed exactly.
struct BracketTicket<'a> {
    entry: Ticket<'a>,
    stop_price: Option<&'a str>,
    trail: Option<&'a str>,
    trail_percent: bool,
    take_profit: Option<&'a str>,
}

fn bracket_request(venue: &str, ticket: &BracketTicket<'_>) -> Result<BracketRequest, String> {
    let entry = new_order(venue, &ticket.entry)?;
    let stop_loss = match (ticket.stop_price, ticket.trail) {
        (Some(_), Some(_)) => {
            return Err("a stop-loss is a price or a trailing distance, not both".to_string())
        }
        (Some(raw), None) => {
            let price = parse_decimal("stop-loss", raw)?;
            Some(match entry.side {
                Side::Buy => Trigger::AtOrBelow(price),
                Side::Sell => Trigger::AtOrAbove(price),
            })
        }
        (None, Some(raw)) => {
            let distance = parse_decimal("trailing distance", raw)?;
            Some(Trigger::Trailing(if ticket.trail_percent {
                Offset::Percent(distance)
            } else {
                Offset::Amount(distance)
            }))
        }
        (None, None) => None,
    };
    let take_profit = ticket
        .take_profit
        .map(|raw| parse_decimal("take-profit", raw))
        .transpose()?;
    Ok(BracketRequest {
        entry,
        stop_loss,
        take_profit,
    })
}

fn place_bracket_order(shell: &Shell, ticket: &BracketTicket<'_>) -> Result<BracketDto, String> {
    let request = bracket_request(&shell.venue, ticket)?;
    let mut exec = lock(&shell.exec)?;
    let (order, exits) = exec.place_bracket(request).map_err(|e| e.to_string())?;
    let placed = BracketDto {
        order: OrderDto::from(&order),
        exits: exits.iter().map(ConditionalDto::from).collect(),
    };
    let mut pending = lock(&shell.pending)?;
    pending.order(placed.order.clone());
    for exit in &placed.exits {
        pending.conditional(exit.clone());
    }
    Ok(placed)
}

/// Sets the trader's switch, for every window. Orders already working stay
/// working: halting cancels nothing by itself.
fn set_trading(shell: &Shell, state: &str) -> Result<(), String> {
    let wanted =
        dto::parse_trading(state).ok_or_else(|| format!("unknown trading state: {state}"))?;
    let mut exec = lock(&shell.exec)?;
    if exec.set_trading_state(wanted) {
        lock(&shell.pending)?.trading(dto::trading_label(wanted));
    }
    Ok(())
}

/// Cancels every open order, or every open order in `symbol`.
fn cancel_all(shell: &Shell, symbol: Option<&str>) -> Result<CancelAllDto, String> {
    let instrument = symbol.map(|symbol| shell.instrument(symbol));
    let mut exec = lock(&shell.exec)?;
    let mut sent = Vec::new();
    let mut failed = Vec::new();
    for (id, result) in exec.cancel_all(instrument.as_ref()) {
        match result {
            Ok(order) => sent.push(OrderDto::from(&order)),
            Err(e) => failed.push(CancelFailureDto {
                id: id.0,
                error: e.to_string(),
            }),
        }
    }
    let mut pending = lock(&shell.pending)?;
    for order in &sent {
        pending.order(order.clone());
    }
    Ok(CancelAllDto { sent, failed })
}

fn amend(
    shell: &Shell,
    id: u64,
    quantity: Decimal,
    limit_price: Decimal,
) -> Result<OrderDto, String> {
    let mut exec = lock(&shell.exec)?;
    let order = exec
        .amend(ClientOrderId(id), quantity, limit_price)
        .map(|o| OrderDto::from(&o))
        .map_err(|e| e.to_string())?;
    lock(&shell.pending)?.order(order.clone());
    Ok(order)
}

#[tauri::command]
fn snapshot(shell: State<'_, Shell>) -> Result<Snapshot, String> {
    snapshot_of(&shell)
}

/// The newest bars of one instrument, oldest first: a chart's history.
#[tauri::command]
fn bars(
    shell: State<'_, Shell>,
    symbol: String,
    limit: Option<usize>,
) -> Result<Vec<BarDto>, String> {
    let limit = limit.unwrap_or(DEFAULT_BAR_LIMIT);
    Ok(lock(&shell.data)?
        .bars(&shell.instrument(&symbol), limit)
        .iter()
        .map(|bar| BarDto::new(&symbol, bar))
        .collect())
}

/// The newest prints of one instrument, oldest first: the history time and
/// sales starts from. Later prints arrive in the batches.
#[tauri::command]
fn prints(
    shell: State<'_, Shell>,
    symbol: String,
    limit: Option<usize>,
) -> Result<Vec<PrintDto>, String> {
    let limit = limit.unwrap_or(DEFAULT_PRINT_LIMIT);
    Ok(lock(&shell.data)?
        .prints(&shell.instrument(&symbol), limit)
        .iter()
        .map(PrintDto::from)
        .collect())
}

/// Returns the order as recorded, including a risk denial: a denied order is
/// a result to show in the blotter, not a failed command.
#[tauri::command]
fn place_order(
    shell: State<'_, Shell>,
    symbol: String,
    side: String,
    quantity: String,
    limit_price: Option<String>,
    kind: Option<String>,
    time_in_force: Option<String>,
) -> Result<OrderDto, String> {
    let ticket = Ticket {
        symbol: &symbol,
        side: &side,
        quantity: &quantity,
        limit_price: limit_price.as_deref(),
        kind: kind.as_deref(),
        time_in_force: time_in_force.as_deref(),
    };
    submit(&shell, new_order(&shell.venue, &ticket)?)
}

/// An entry order with its exits (ADR 0022): the entry as `place_order`
/// takes it; a stop-loss at `stop_price`, or trailing by `trail` (in
/// percent with `trail_percent`); a take-profit at `take_profit`. The exits
/// wait for the entry's fills. A denied entry comes back with no exits.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
fn place_bracket(
    shell: State<'_, Shell>,
    symbol: String,
    side: String,
    quantity: String,
    limit_price: Option<String>,
    kind: Option<String>,
    time_in_force: Option<String>,
    stop_price: Option<String>,
    trail: Option<String>,
    trail_percent: Option<bool>,
    take_profit: Option<String>,
) -> Result<BracketDto, String> {
    place_bracket_order(
        &shell,
        &BracketTicket {
            entry: Ticket {
                symbol: &symbol,
                side: &side,
                quantity: &quantity,
                limit_price: limit_price.as_deref(),
                kind: kind.as_deref(),
                time_in_force: time_in_force.as_deref(),
            },
            stop_price: given(&stop_price),
            trail: given(&trail),
            trail_percent: trail_percent.unwrap_or(false),
            take_profit: given(&take_profit),
        },
    )
}

/// How large one order could be now (ADR 0016): what the account can pay
/// for or holds, within this terminal's limits, in the quantities the market
/// takes. The ticket asks as its side, type, and price change. A buy without
/// a limit price is valued at the day's ceiling.
#[tauri::command]
fn order_capacity(
    shell: State<'_, Shell>,
    symbol: String,
    side: String,
    limit_price: Option<String>,
    kind: Option<String>,
    time_in_force: Option<String>,
) -> Result<CapacityDto, String> {
    let ticket = Ticket {
        symbol: &symbol,
        side: &side,
        quantity: "0",
        limit_price: limit_price.as_deref().filter(|raw| !raw.trim().is_empty()),
        kind: kind.as_deref(),
        time_in_force: time_in_force.as_deref(),
    };
    capacity_of(&shell, &ticket)
}

fn capacity_of(shell: &Shell, ticket: &Ticket<'_>) -> Result<CapacityDto, String> {
    let order = new_order(&shell.venue, ticket)?;
    let exec = lock(&shell.exec)?;
    Ok(CapacityDto::from(&exec.capacity(
        &order.instrument,
        order.side,
        order.order_type,
        order.limit_price,
    )))
}

#[tauri::command]
fn cancel_order(shell: State<'_, Shell>, id: u64) -> Result<OrderDto, String> {
    cancel(&shell, id)
}

/// Every open order, or every open order in `symbol`: what was sent and
/// what could not be. Nothing to cancel is an empty result, not an error.
#[tauri::command]
fn cancel_all_orders(
    shell: State<'_, Shell>,
    symbol: Option<String>,
) -> Result<CancelAllDto, String> {
    cancel_all(&shell, symbol.as_deref())
}

/// Halts trading, takes only orders that reduce a position, or lets it run
/// again: `active`, `reducing`, or `halted`. Orders already working are
/// not touched.
#[tauri::command]
fn set_trading_state(shell: State<'_, Shell>, state: String) -> Result<(), String> {
    set_trading(&shell, &state)
}

/// How large an order may be for what the trader will lose if its stop is
/// hit (ADR 0021); see `SizingTicket` for the fields.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
fn size_for_risk(
    shell: State<'_, Shell>,
    symbol: String,
    side: String,
    kind: Option<String>,
    time_in_force: Option<String>,
    entry: String,
    stop: String,
    risk: String,
    percent: Option<bool>,
) -> Result<SizingDto, String> {
    sizing_of(
        &shell,
        &SizingTicket {
            symbol: &symbol,
            side: &side,
            kind: kind.as_deref(),
            time_in_force: time_in_force.as_deref(),
            entry: &entry,
            stop: &stop,
            risk: &risk,
            percent: percent.unwrap_or(false),
        },
    )
}

/// Holds an order in this terminal until a trade reaches its trigger
/// (ADR 0020); see `ConditionalTicket` for the fields.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
fn place_conditional(
    shell: State<'_, Shell>,
    symbol: String,
    side: String,
    quantity: String,
    trigger: String,
    price: Option<String>,
    offset: Option<String>,
    percent: Option<bool>,
    limit_price: Option<String>,
) -> Result<ConditionalDto, String> {
    place_conditional_order(
        &shell,
        &ConditionalTicket {
            symbol: &symbol,
            side: &side,
            quantity: &quantity,
            trigger: &trigger,
            price: price.as_deref(),
            offset: offset.as_deref(),
            percent: percent.unwrap_or(false),
            limit_price: limit_price.as_deref().filter(|raw| !raw.trim().is_empty()),
        },
    )
}

#[tauri::command]
fn cancel_conditional(shell: State<'_, Shell>, id: u64) -> Result<ConditionalDto, String> {
    cancel_conditional_order(&shell, id)
}

/// A field the ticket left empty is no field at all.
fn given(raw: &Option<String>) -> Option<&str> {
    raw.as_deref().filter(|raw| !raw.trim().is_empty())
}

/// A resting limit order's new total quantity and price. The new terms pass
/// risk as a new order would; the venue's answer arrives in a later batch.
#[tauri::command]
fn amend_order(
    shell: State<'_, Shell>,
    id: u64,
    quantity: String,
    limit_price: String,
) -> Result<OrderDto, String> {
    amend(
        &shell,
        id,
        parse_decimal("quantity", &quantity)?,
        parse_decimal("limit price", &limit_price)?,
    )
}

/// Routes one bus event to the engine that owns it and queues what changed
/// for the front end. Locks follow the order documented on [`Shell`].
fn route(shell: &Shell, event: &Event) -> Result<(), String> {
    match event {
        Event::Quote(quote) => {
            let update = {
                let mut data = lock(&shell.data)?;
                data.on_quote(quote).then(|| {
                    let symbol = &quote.instrument.symbol;
                    let bar = data
                        .last_bar(&quote.instrument)
                        .map(|bar| BarDto::new(symbol, bar));
                    let board = BoardDto::of(&data, &quote.instrument);
                    (QuoteDto::from(quote), bar, board)
                })
            };
            if let Some((quote, bar, board)) = update {
                let mut pending = lock(&shell.pending)?;
                pending.quote(quote);
                pending.board(board);
                if let Some(bar) = bar {
                    pending.bar(bar);
                }
            }
        }
        Event::Depth(depth) => {
            let board = {
                let mut data = lock(&shell.data)?;
                data.on_depth(depth)
                    .then(|| BoardDto::of(&data, &depth.instrument))
            };
            if let Some(board) = board {
                lock(&shell.pending)?.board(board);
            }
        }
        Event::Trade(trade) => {
            let update = {
                let mut data = lock(&shell.data)?;
                data.on_trade(trade).then(|| {
                    let print = data.last_print(&trade.instrument).map(PrintDto::from);
                    (print, BoardDto::of(&data, &trade.instrument))
                })
            };
            if let Some((print, board)) = update {
                let mut pending = lock(&shell.pending)?;
                if let Some(print) = print {
                    pending.print(print);
                }
                pending.board(board);
            }
            // Conditional orders watch the tape (ADR 0020).
            if shell.waiting.load(Ordering::Acquire) > 0 {
                fire_conditionals(shell, &trade.instrument, trade.price)?;
            }
        }
        // Reference data goes to both engines: the data engine to show it,
        // the execution engine to check orders against it.
        Event::Instrument(info) => {
            let board = {
                let mut data = lock(&shell.data)?;
                data.on_instrument(info);
                BoardDto::of(&data, &info.id)
            };
            lock(&shell.exec)?.set_instrument(info.clone());
            let mut pending = lock(&shell.pending)?;
            pending.instrument(InstrumentDto::from(info));
            pending.board(board);
        }
        Event::PriceBand(band) => {
            let board = {
                let mut data = lock(&shell.data)?;
                data.on_band(band);
                BoardDto::of(&data, &band.instrument)
            };
            lock(&shell.exec)?.set_band(band.clone());
            lock(&shell.pending)?.board(board);
        }
        Event::Execution(report) => {
            let mut exec = lock(&shell.exec)?;
            match exec.apply(report) {
                Ok(order) => {
                    // A fill wakes the exits that wait on this order (ADR 0022).
                    let exits = exec.on_order(&order);
                    shell
                        .waiting
                        .store(exec.waiting_conditionals(), Ordering::Release);
                    let mut pending = lock(&shell.pending)?;
                    pending.order(OrderDto::from(&order));
                    for exit in &exits {
                        pending.conditional(ConditionalDto::from(exit));
                    }
                }
                Err(e) => eprintln!(
                    "execution report for order {} ignored: {e}",
                    report.order_id
                ),
            }
        }
        // The execution engine keeps the account: risk checks sells against it.
        Event::AccountUpdate(account) => {
            lock(&shell.exec)?.set_account(account.clone());
            lock(&shell.pending)?.account(AccountDto::from(account));
        }
        // A session goes to both engines: the data engine to show it, the
        // execution engine to check orders against what the market takes.
        Event::Session(session) => {
            lock(&shell.data)?.on_session(session);
            lock(&shell.exec)?.set_session(session.clone());
            lock(&shell.pending)?.session(SessionDto::from(session));
        }
        // Each engine keeps the status of its own connection.
        Event::Connection(status) => {
            let kept_by_data = lock(&shell.data)?.on_connection(status);
            if kept_by_data || lock(&shell.exec)?.on_connection(status) {
                lock(&shell.pending)?.connection(ConnectionDto::from(status));
            }
        }
        _ => {}
    }
    Ok(())
}

fn pump(app: AppHandle, events: Receiver<Event>) {
    thread::spawn(move || {
        let shell = app.state::<Shell>();
        for event in events {
            if let Err(e) = route(&shell, &event) {
                eprintln!("event pump stopped: {e}");
                return;
            }
        }
    });
}

/// Sends whatever changed since the last flush, as one message.
fn flush(app: AppHandle) {
    thread::spawn(move || {
        let shell = app.state::<Shell>();
        loop {
            thread::sleep(FLUSH_INTERVAL);
            match take_batch(&shell) {
                Ok(Some(batch)) => {
                    let _ = app.emit(EVENT_NAME, batch);
                }
                Ok(None) => {}
                Err(e) => {
                    eprintln!("front-end updates stopped: {e}");
                    return;
                }
            }
        }
    });
}

fn connect_engines(bus: &Bus, venue: Venue) -> Result<Shell, AdapterError> {
    // The front end names instruments by symbol and the shell adds the
    // venue id, so an instrument on another venue could never be shown.
    if let Some(stray) = venue.instruments.iter().find(|i| i.venue != venue.id) {
        return Err(AdapterError::fatal(format!(
            "{stray} is not on the venue being run, \"{}\"",
            venue.id
        )));
    }
    let mut data = DataEngine::new(venue.data);
    data.connect(bus.handle())?;
    for instrument in &venue.instruments {
        data.subscribe_quotes(instrument)?;
        data.subscribe_trades(instrument)?;
    }

    let mut exec = ExecutionEngine::new(venue.execution, venue.limits);
    exec.connect(bus.handle())?;

    Ok(Shell {
        venue: venue.id,
        market: MarketDto {
            id: venue.market.id,
            title: venue.market.title,
            currency: venue.market.currency,
        },
        data: Mutex::new(data),
        exec: Mutex::new(exec),
        pending: Mutex::new(Batch::default()),
        flushed: AtomicU64::new(0),
        waiting: AtomicUsize::new(0),
    })
}

/// Connects `venue` and runs the terminal until its last window closes.
pub fn run(venue: Venue) -> ExitCode {
    let bus = Bus::new();
    let events = bus.subscribe();
    let shell = match connect_engines(&bus, venue) {
        Ok(shell) => shell,
        Err(e) => {
            eprintln!("could not connect to the venue: {e}");
            return ExitCode::FAILURE;
        }
    };

    let app = tauri::Builder::default()
        .plugin(
            tauri_plugin_window_state::Builder::default()
                // Whether a window has a frame is the app's call, not a saved
                // state's: a window saved with one must not get it back.
                .with_state_flags(StateFlags::all().difference(StateFlags::DECORATIONS))
                .build(),
        )
        .setup(move |app| {
            app.manage(shell);
            workspace::setup(app)?;
            pump(app.handle().clone(), events);
            flush(app.handle().clone());
            Ok(())
        })
        .on_window_event(workspace::on_window_event)
        .invoke_handler(tauri::generate_handler![
            snapshot,
            bars,
            prints,
            place_order,
            order_capacity,
            cancel_order,
            cancel_all_orders,
            amend_order,
            set_trading_state,
            place_conditional,
            cancel_conditional,
            size_for_risk,
            place_bracket,
            workspace::settings_get,
            workspace::settings_set,
            workspace::settings_remove,
            workspace::open_window
        ])
        .build(context());

    match app {
        Ok(app) => {
            app.run(workspace::on_run_event);
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("Fansipan could not start: {e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use execution_engine::ConditionalError;
    use model::{Quote, UnixNanos};
    use sandbox::{MarketProfile, SandboxDataClient, SandboxExecutionClient};

    const VENUE: &str = "sim";

    fn profile(id: &str) -> MarketProfile {
        MarketProfile::by_id(id, VENUE).unwrap()
    }

    /// The sandbox, wired as `src/main.rs` wires it.
    fn sandbox_venue(profile: &MarketProfile) -> Venue {
        let sim = profile.venue();
        Venue::new(
            VENUE,
            Market::new(profile.id, profile.title, profile.currency),
            Box::new(SandboxDataClient::new(sim.clone())),
            Box::new(SandboxExecutionClient::new(sim)),
            RiskLimits {
                max_order_quantity: profile.max_order_quantity,
                max_order_notional: profile.max_order_notional,
            },
        )
        .subscribe(profile.listings.iter().map(|l| l.info.id.clone()))
    }

    fn connect(bus: &Bus, profile: &MarketProfile) -> Shell {
        connect_engines(bus, sandbox_venue(profile)).unwrap()
    }

    fn shell() -> Shell {
        connect(&Bus::new(), &profile("us"))
    }

    /// Routes the reference data the sandbox would announce on its first
    /// beat, as the pump does.
    fn announce(shell: &Shell, profile: &MarketProfile) {
        for event in profile.venue().reference_data(UnixNanos(0)) {
            route(shell, &event).unwrap();
        }
    }

    fn take_pending(shell: &Shell) -> BatchDto {
        take_batch(shell).unwrap().unwrap_or(BatchDto {
            seq: 0,
            quotes: vec![],
            orders: vec![],
            account: None,
            bars: vec![],
            instruments: vec![],
            boards: vec![],
            connections: vec![],
            sessions: vec![],
            prints: vec![],
            trading: None,
            day: None,
            conditionals: vec![],
        })
    }

    /// A ticket with no order type named, as the ticket sends one today.
    fn ticket<'a>(
        symbol: &'a str,
        side: &'a str,
        quantity: &'a str,
        limit_price: Option<&'a str>,
    ) -> Ticket<'a> {
        Ticket {
            symbol,
            side,
            quantity,
            limit_price,
            kind: None,
            time_in_force: None,
        }
    }

    #[test]
    fn a_market_order_reaches_the_batch_as_filled() {
        let bus = Bus::new();
        let events = bus.subscribe();
        let shell = connect(&bus, &profile("us"));

        let placed = submit(
            &shell,
            new_order(VENUE, &ticket("AAPL", "buy", "1", None)).unwrap(),
        )
        .unwrap();
        for event in events.try_iter() {
            route(&shell, &event).unwrap();
        }

        let batch = take_pending(&shell);
        let order = batch.orders.iter().find(|o| o.id == placed.id).unwrap();
        assert_eq!(order.status, "filled");
    }

    #[test]
    fn a_fill_reaches_the_batch_with_the_day_it_made() {
        let bus = Bus::new();
        let events = bus.subscribe();
        let shell = connect(&bus, &profile("us"));
        announce(&shell, &profile("us"));

        submit(
            &shell,
            new_order(VENUE, &ticket("AAPL", "buy", "1", None)).unwrap(),
        )
        .unwrap();
        for event in events.try_iter() {
            route(&shell, &event).unwrap();
        }
        // A price to mark it at, however far the venue's account report
        // has got: held, it is marked; not yet, it counts from the band.
        let print = Event::Trade(model::Trade {
            instrument: InstrumentId::new(VENUE, "AAPL"),
            price: Decimal::from(190),
            quantity: Decimal::ONE,
            side: None,
            ts_event: UnixNanos(1),
        });
        route(&shell, &print).unwrap();

        let day = take_pending(&shell)
            .day
            .expect("every batch carries the day");
        let line = day.instruments.iter().find(|l| l.symbol == "AAPL").unwrap();
        assert!(line.pnl.is_some(), "{day:?}");
        assert_eq!(
            snapshot_of(&shell).unwrap().day.instruments.len(),
            day.instruments.len()
        );
    }

    #[test]
    fn the_days_total_adds_what_is_known_and_says_what_is_not() {
        let day = |symbol: &str, pnl: Option<&str>, costs: &str| execution_engine::DayPnl {
            instrument: InstrumentId::new(VENUE, symbol),
            pnl: pnl.map(|p| p.parse().unwrap()),
            costs: costs.parse().unwrap(),
        };

        let dto = DayDto::of(&[
            day("A", Some("10.5"), "1"),
            day("B", None, "2"),
            day("C", Some("-4"), "0"),
        ]);

        assert_eq!(
            (dto.total.as_str(), dto.costs.as_str(), dto.partial),
            ("6.5", "3", true)
        );
        assert_eq!(dto.instruments[1].pnl, None);
    }

    fn buy_above(price: &'static str) -> ConditionalTicket<'static> {
        ConditionalTicket {
            symbol: "AAPL",
            side: "buy",
            quantity: "1",
            trigger: "at_or_above",
            price: Some(price),
            offset: None,
            percent: false,
            limit_price: None,
        }
    }

    #[test]
    fn a_conditional_fires_on_the_tape_and_reaches_the_batch_with_its_order() {
        let shell = shell();
        announce(&shell, &profile("us"));
        let placed = place_conditional_order(&shell, &buy_above("100")).unwrap();
        take_pending(&shell);

        let trade = Event::Trade(model::Trade {
            instrument: InstrumentId::new(VENUE, "AAPL"),
            price: Decimal::from(150),
            quantity: Decimal::ONE,
            side: None,
            ts_event: UnixNanos(1),
        });
        route(&shell, &trade).unwrap();
        let batch = take_pending(&shell);

        let fired = batch
            .conditionals
            .iter()
            .find(|c| c.id == placed.id)
            .unwrap();
        assert_eq!(fired.status, "fired");
        let order = fired.order_id.expect("the order it sent");
        assert!(batch.orders.iter().any(|o| o.id == order));
        assert_eq!(snapshot_of(&shell).unwrap().conditionals[0].status, "fired");
        assert_eq!(
            shell.waiting.load(Ordering::Acquire),
            0,
            "trades skip the engine again"
        );
        assert!(
            place_conditional_order(&shell, &buy_above("100")).is_err(),
            "the last trade, 150, is past it"
        );
        assert_eq!(
            shell.waiting.load(Ordering::Acquire),
            0,
            "a refusal is not counted"
        );
    }

    #[test]
    fn the_window_has_an_icon_in_a_program_without_the_shells_resources() {
        // A test executable carries no icon resources, as an adapter's
        // example that opens the terminal does not.
        assert!(context().default_window_icon().is_some());
    }

    fn size_aapl(stop: &'static str) -> SizingTicket<'static> {
        SizingTicket {
            symbol: "AAPL",
            side: "buy",
            kind: Some("limit"),
            time_in_force: Some("day"),
            entry: "150",
            stop,
            risk: "500",
            percent: false,
        }
    }

    #[test]
    fn a_size_from_risk_is_parsed_exactly_and_says_what_is_wrong() {
        let shell = shell();
        announce(&shell, &profile("us"));

        let sizing = sizing_of(&shell, &size_aapl("145")).unwrap();
        assert_eq!(
            (sizing.budget.as_str(), sizing.loss_per_unit.as_str()),
            ("500", "5")
        );
        assert!(sizing.at_risk.parse::<Decimal>().unwrap() <= Decimal::from(500));

        assert_eq!(
            sizing_of(&shell, &size_aapl("155")).unwrap_err(),
            "a buy's stop must be below its entry"
        );
        assert!(sizing_of(&shell, &size_aapl("abc")).is_err());
    }

    fn aapl_bracket(
        stop: Option<&'static str>,
        trail: Option<&'static str>,
    ) -> BracketTicket<'static> {
        BracketTicket {
            entry: Ticket {
                symbol: "AAPL",
                side: "buy",
                quantity: "10",
                limit_price: Some("100"),
                kind: Some("limit"),
                time_in_force: Some("day"),
            },
            stop_price: stop,
            trail,
            trail_percent: false,
            take_profit: Some("120"),
        }
    }

    #[test]
    fn a_brackets_exits_wake_when_the_venue_reports_its_entry_filled() {
        let shell = shell();
        announce(&shell, &profile("us"));
        let placed = place_bracket_order(&shell, &aapl_bracket(Some("95"), None)).unwrap();
        let first = take_pending(&shell);
        assert_eq!(placed.exits.len(), 2);
        assert!(placed
            .exits
            .iter()
            .all(|exit| exit.status == "pending" && exit.after_order == Some(placed.order.id)));
        assert_eq!(first.conditionals.len(), 2);
        assert_eq!(shell.waiting.load(Ordering::Acquire), 0);

        let id = ClientOrderId(placed.order.id);
        for event in [
            model::OrderEvent::Accepted {
                venue_order_id: model::VenueOrderId("V1".into()),
            },
            model::OrderEvent::Filled {
                fill_id: model::FillId("F1".into()),
                quantity: Decimal::from(4),
                price: Decimal::from(100),
            },
        ] {
            let report = model::ExecutionReport {
                order_id: id,
                event,
                ts_event: UnixNanos(1),
            };
            route(&shell, &Event::Execution(report)).unwrap();
        }
        let batch = take_pending(&shell);

        assert!(batch
            .conditionals
            .iter()
            .all(|exit| exit.status == "waiting" && exit.quantity == "4"));
        assert_eq!(
            shell.waiting.load(Ordering::Acquire),
            2,
            "trades reach them now"
        );
    }

    #[test]
    fn a_bracket_is_parsed_exactly_and_says_what_is_wrong() {
        let shell = shell();
        assert_eq!(
            place_bracket_order(&shell, &aapl_bracket(Some("95"), Some("2"))).unwrap_err(),
            "a stop-loss is a price or a trailing distance, not both"
        );
        assert_eq!(
            place_bracket_order(&shell, &aapl_bracket(Some("105"), None)).unwrap_err(),
            ConditionalError::ExitOnWrongSide.to_string()
        );
        let trailing = place_bracket_order(&shell, &aapl_bracket(None, Some("2"))).unwrap();
        assert_eq!(
            (
                trailing.exits[0].trigger,
                trailing.exits[0].offset.as_deref()
            ),
            ("trailing", Some("2"))
        );
    }

    #[test]
    fn a_conditional_is_parsed_exactly_and_can_be_cancelled() {
        let shell = shell();
        let wrong = ConditionalTicket {
            trigger: "sometime",
            ..buy_above("100")
        };

        assert!(place_conditional_order(&shell, &wrong).is_err());
        let placed = place_conditional_order(&shell, &buy_above("100")).unwrap();
        assert_eq!(
            (placed.trigger, placed.price.as_deref(), placed.then),
            ("at_or_above", Some("100"), "marketable")
        );
        assert_eq!(shell.waiting.load(Ordering::Acquire), 1);
        assert_eq!(
            cancel_conditional_order(&shell, placed.id).unwrap().status,
            "cancelled"
        );
        assert_eq!(shell.waiting.load(Ordering::Acquire), 0);
        assert!(cancel_conditional_order(&shell, placed.id).is_err());
    }

    #[test]
    fn halting_reaches_every_window_and_stops_new_orders() {
        let shell = shell();
        let order = || new_order(VENUE, &ticket("AAPL", "buy", "1", Some("1.00"))).unwrap();
        let working = submit(&shell, order()).unwrap();
        take_pending(&shell);

        set_trading(&shell, "halted").unwrap();
        let batch = take_pending(&shell);
        let denied = submit(&shell, order()).unwrap();

        assert_eq!(batch.trading, Some("halted"));
        assert_eq!(snapshot_of(&shell).unwrap().trading, "halted");
        assert_eq!(denied.status, "denied");
        assert!(
            cancel(&shell, working.id).is_ok(),
            "cancels are never stopped"
        );
        assert!(set_trading(&shell, "paused").is_err());
    }

    #[test]
    fn batches_are_numbered_and_a_snapshot_carries_the_last_number() {
        let shell = shell();
        shell.pending.lock().unwrap().account(AccountDto {
            cash: "1".into(),
            positions: vec![],
            buying_power: None,
        });

        let first = take_batch(&shell).unwrap().map(|b| b.seq);
        let nothing = take_batch(&shell).unwrap();
        let snapshot = snapshot_of(&shell).unwrap();

        assert_eq!(first, Some(1));
        assert!(nothing.is_none());
        assert_eq!(snapshot.seq, 1);
    }

    #[test]
    fn order_input_is_parsed_exactly() {
        let order = new_order(VENUE, &ticket("AAPL", "buy", " 10 ", Some("190.10"))).unwrap();

        assert_eq!(order.quantity, Decimal::TEN);
        assert_eq!(order.limit_price, Some(Decimal::new(19_010, 2)));
        assert_eq!(order.instrument, InstrumentId::new(VENUE, "AAPL"));
    }

    #[test]
    fn market_orders_have_no_limit_price() {
        let order = new_order(VENUE, &ticket("AAPL", "sell", "1", None)).unwrap();

        assert_eq!(order.side, Side::Sell);
        assert_eq!(order.limit_price, None);
    }

    #[test]
    fn malformed_input_is_refused_with_the_field_named() {
        assert_eq!(
            new_order(VENUE, &ticket("AAPL", "buy", "ten", None)).unwrap_err(),
            "quantity: \"ten\" is not a number"
        );
        assert_eq!(
            new_order(VENUE, &ticket("AAPL", "buy", "1", Some(""))).unwrap_err(),
            "limit price: \"\" is not a number"
        );
        assert_eq!(
            new_order(VENUE, &ticket("AAPL", "hold", "1", None)).unwrap_err(),
            "unknown side: hold"
        );
    }

    #[test]
    fn a_report_for_an_unknown_order_queues_nothing() {
        let shell = shell();
        let report = Event::Execution(model::ExecutionReport {
            order_id: ClientOrderId(404),
            event: model::OrderEvent::Cancelled,
            ts_event: UnixNanos(0),
        });

        route(&shell, &report).unwrap();

        assert!(take_pending(&shell).orders.is_empty());
    }

    #[test]
    fn a_quote_queues_itself_and_the_bar_it_moved() {
        let shell = shell();
        let quote = Event::Quote(Quote {
            instrument: InstrumentId::new(VENUE, "AAPL"),
            bid: Decimal::new(19_000, 2),
            ask: Decimal::new(19_002, 2),
            bid_size: Decimal::ONE,
            ask_size: Decimal::ONE,
            ts_event: UnixNanos(1_700_000_003_000_000_000),
        });

        route(&shell, &quote).unwrap();

        let batch = take_pending(&shell);
        assert_eq!(batch.quotes.len(), 1);
        assert_eq!(batch.bars.len(), 1);
        assert_eq!(batch.bars[0].close, "190.01");
        assert_eq!(batch.bars[0].time_ms, 1_700_000_000_000);
        assert_eq!(batch.boards[0].bids[0].price, "190");
    }

    #[test]
    fn reference_data_reaches_the_board_and_the_snapshot() {
        let vn = profile("vn");
        let shell = connect(&Bus::new(), &vn);

        announce(&shell, &vn);

        let batch = take_pending(&shell);
        let vnm = batch.boards.iter().find(|b| b.symbol == "VNM").unwrap();
        assert_eq!(vnm.exchange, "HOSE");
        assert_eq!(vnm.reference.as_deref(), Some("62500"));
        assert_eq!(vnm.ceiling.as_deref(), Some("66800"));
        assert_eq!(vnm.floor.as_deref(), Some("58200"));
        let info = batch
            .instruments
            .iter()
            .find(|i| i.symbol == "VNM")
            .unwrap();
        assert_eq!(info.lot_size, "100");
        assert_eq!(info.ticks.len(), 3);

        let snapshot = snapshot_of(&shell).unwrap();
        assert_eq!(snapshot.market.currency, "VND");
        assert_eq!(snapshot.instruments.len(), vn.listings.len());
        assert_eq!(snapshot.boards.len(), vn.listings.len());
    }

    #[test]
    fn the_ticket_learns_how_large_one_order_can_be() {
        let vn = profile("vn");
        let shell = connect(&Bus::new(), &vn);
        announce(&shell, &vn);
        route(&shell, &Event::AccountUpdate(vn.venue().account())).unwrap();

        let buy = capacity_of(&shell, &ticket("VNM", "buy", "0", Some("62500"))).unwrap();
        let market_buy = capacity_of(&shell, &ticket("VNM", "buy", "0", None)).unwrap();
        let sell = capacity_of(&shell, &ticket("VNM", "sell", "0", Some("62500"))).unwrap();

        // The venue's 2,000,000,000 VND of buying power, at 62,500 a share.
        assert_eq!(buy.max_quantity.as_deref(), Some("32000"));
        assert_eq!(buy.bound, Some("buying_power"));
        assert!(!buy.buying_power_estimated);
        // Without a price, a buy is valued at the day's ceiling: 29,940 shares
        // fit, and whole lots make 29,900.
        assert_eq!(market_buy.valued_at.as_deref(), Some("66800"));
        assert_eq!(market_buy.max_quantity.as_deref(), Some("29900"));
        // Nothing held, and Vietnam forbids short selling.
        assert_eq!(sell.max_quantity.as_deref(), Some("0"));
        assert_eq!(sell.bound, Some("holding"));
    }

    #[test]
    fn announced_rules_deny_an_order_before_it_reaches_the_venue() {
        let vn = profile("vn");
        let shell = connect(&Bus::new(), &vn);
        announce(&shell, &vn);

        let odd_lot = submit(
            &shell,
            new_order(VENUE, &ticket("VNM", "buy", "150", Some("62500"))).unwrap(),
        );
        let above_ceiling = submit(
            &shell,
            new_order(VENUE, &ticket("VNM", "buy", "100", Some("66900"))).unwrap(),
        );

        assert_eq!(odd_lot.unwrap().status, "denied");
        assert_eq!(above_ceiling.unwrap().status, "denied");
    }

    #[test]
    fn a_trade_moves_the_board_change_against_the_reference() {
        let vn = profile("vn");
        let shell = connect(&Bus::new(), &vn);
        announce(&shell, &vn);
        take_pending(&shell);

        route(
            &shell,
            &Event::Trade(model::Trade {
                instrument: InstrumentId::new(VENUE, "VNM"),
                price: Decimal::new(63_100, 0),
                quantity: Decimal::new(300, 0),
                side: Some(Side::Buy),
                ts_event: UnixNanos(1),
            }),
        )
        .unwrap();

        let board = take_pending(&shell).boards.remove(0);
        assert_eq!(board.last.as_deref(), Some("63100"));
        assert_eq!(board.change.as_deref(), Some("600"));
        assert_eq!(board.change_pct.as_deref(), Some("0.96"));
        assert_eq!(board.volume, "300");
    }

    #[test]
    fn a_trade_reaches_the_batch_and_the_tape_as_a_numbered_print() {
        let vn = profile("vn");
        let shell = connect(&Bus::new(), &vn);
        take_pending(&shell);
        let trade = |price: i64, side| {
            Event::Trade(model::Trade {
                instrument: InstrumentId::new(VENUE, "VNM"),
                price: Decimal::new(price, 0),
                quantity: Decimal::new(300, 0),
                side,
                ts_event: UnixNanos(2_000_000),
            })
        };

        route(&shell, &trade(63_100, Some(Side::Buy))).unwrap();
        route(&shell, &trade(63_000, None)).unwrap();

        let batch = take_pending(&shell);
        let sent: Vec<(u64, &str, Option<&str>, u64)> = batch
            .prints
            .iter()
            .map(|p| (p.seq, p.price.as_str(), p.side, p.time_ms))
            .collect();
        assert_eq!(
            sent,
            vec![(1, "63100", Some("buy"), 2), (2, "63000", None, 2)]
        );
        let tape: Vec<PrintDto> = lock(&shell.data)
            .unwrap()
            .prints(&shell.instrument("VNM"), DEFAULT_PRINT_LIMIT)
            .iter()
            .map(PrintDto::from)
            .collect();
        assert_eq!(tape, batch.prints);
    }

    #[test]
    fn orders_and_bars_use_the_venue_the_adapter_names() {
        let shell = connect_engines(&Bus::new(), sandbox_venue(&profile("us"))).unwrap();

        assert_eq!(shell.instrument("AAPL"), InstrumentId::new(VENUE, "AAPL"));
    }

    #[test]
    fn an_instrument_on_another_venue_is_refused() {
        let venue = sandbox_venue(&profile("us")).subscribe([InstrumentId::new("elsewhere", "X")]);

        let err = connect_engines(&Bus::new(), venue).err().expect("refused");

        assert!(err.message.contains("elsewhere"));
    }

    #[test]
    fn subscribing_adds_to_what_was_there() {
        let venue = sandbox_venue(&profile("us"));
        let before = venue.instruments().len();

        let venue = venue.subscribe([InstrumentId::new(VENUE, "EXTRA")]);

        assert_eq!(venue.instruments().len(), before + 1);
        assert_eq!(venue.id(), VENUE);
    }

    #[test]
    fn a_connection_report_reaches_the_batch_and_the_snapshot() {
        let shell = shell();
        let lost = model::ConnectionStatus::new(
            Connection::Orders,
            model::ConnectionState::Reconnecting,
            UnixNanos(1),
        )
        .with_detail("stream closed");

        route(&shell, &Event::Connection(lost)).unwrap();

        let reported = ConnectionDto {
            connection: "orders",
            state: "reconnecting",
            detail: Some("stream closed".into()),
        };
        assert_eq!(take_pending(&shell).connections, vec![reported.clone()]);
        let up = ConnectionDto {
            connection: "marketData",
            state: "up",
            detail: None,
        };
        assert_eq!(snapshot_of(&shell).unwrap().connections, vec![up, reported]);
    }

    #[test]
    fn the_ticket_can_name_any_order_type_and_defaults_by_price() {
        let ato = Ticket {
            kind: Some("market"),
            time_in_force: Some("atOpen"),
            ..ticket("VNM", "buy", "100", None)
        };
        let typed = new_order(VENUE, &ato).unwrap();
        let limit = new_order(VENUE, &ticket("VNM", "buy", "100", Some("62500"))).unwrap();
        let odd = Ticket {
            kind: Some("stop"),
            ..ticket("VNM", "buy", "100", None)
        };

        assert_eq!(
            typed.order_type,
            model::OrderType::new(model::OrderKind::Market, model::TimeInForce::AtOpen)
        );
        assert_eq!(limit.order_type, model::OrderType::LIMIT);
        assert_eq!(
            new_order(VENUE, &odd).unwrap_err(),
            "unknown order type: stop day"
        );
    }

    #[test]
    fn a_session_reaches_the_batch_and_the_snapshot_and_gates_orders() {
        let vn = profile("vn");
        let shell = connect(&Bus::new(), &vn);
        announce(&shell, &vn);
        take_pending(&shell);
        let closed = model::SessionStatus::new("HOSE", model::SessionPhase::Closed, UnixNanos(1));

        route(&shell, &Event::Session(closed)).unwrap();

        let session = take_pending(&shell).sessions.remove(0);
        assert_eq!((session.market.as_str(), session.phase), ("HOSE", "closed"));
        assert_eq!(snapshot_of(&shell).unwrap().sessions, vec![session]);
        let denied = submit(
            &shell,
            new_order(VENUE, &ticket("VNM", "buy", "100", Some("62500"))).unwrap(),
        )
        .unwrap();
        assert_eq!(denied.status, "denied");
    }

    #[test]
    fn cancel_all_cancels_a_symbols_orders_or_every_order() {
        let shell = connect(&Bus::new(), &profile("us"));
        let place = |symbol: &str| {
            submit(
                &shell,
                new_order(VENUE, &ticket(symbol, "buy", "1", Some("100"))).unwrap(),
            )
            .unwrap()
            .id
        };
        let apple = place("AAPL");
        let tesla = place("TSLA");
        take_pending(&shell);

        let one = cancel_all(&shell, Some("AAPL")).unwrap();
        let ids = |dto: &CancelAllDto| dto.sent.iter().map(|o| o.id).collect::<Vec<_>>();
        assert_eq!(ids(&one), vec![apple]);
        assert_eq!(one.sent[0].status, "cancelling");
        assert!(one.failed.is_empty());
        assert_eq!(take_pending(&shell).orders, one.sent);

        let rest = cancel_all(&shell, None).unwrap();
        assert_eq!(ids(&rest), vec![tesla]);
        assert!(cancel_all(&shell, None).unwrap().sent.is_empty());
    }

    #[test]
    fn an_amendment_reaches_the_venue_and_its_answer_the_batch() {
        let bus = Bus::new();
        let events = bus.subscribe();
        let shell = connect(&bus, &profile("us"));
        let placed = submit(
            &shell,
            new_order(VENUE, &ticket("AAPL", "buy", "1", Some("100"))).unwrap(),
        )
        .unwrap();
        for event in events.try_iter() {
            route(&shell, &event).unwrap();
        }

        let pending = amend(&shell, placed.id, Decimal::TWO, Decimal::new(101, 0)).unwrap();
        for event in events.try_iter() {
            route(&shell, &event).unwrap();
        }

        assert!(snapshot_of(&shell).unwrap().can_amend);
        assert_eq!(pending.status, "amending");
        assert!(!pending.amendable);
        let batch = take_pending(&shell);
        let order = batch.orders.iter().find(|o| o.id == placed.id).unwrap();
        assert_eq!(
            (
                order.status,
                order.quantity.as_str(),
                order.limit_price.as_deref()
            ),
            ("accepted", "2", Some("101"))
        );
        assert!(order.amendable);
        assert!(amend(&shell, 404, Decimal::ONE, Decimal::ONE)
            .unwrap_err()
            .contains("no order 404"));
    }
}
