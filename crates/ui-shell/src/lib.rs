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
mod settings;
mod workspace;

use std::collections::BTreeSet;
use std::mem;
use std::process::ExitCode;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::{Mutex, MutexGuard};
use std::thread;
use std::time::Duration;

use adapter_api::{AdapterError, DataClient, ExecutionClient};
use bus::{Bus, Event};
use data_engine::DataEngine;
use execution_engine::ExecutionEngine;
use model::{ClientOrderId, Connection, Decimal, InstrumentId, NewOrder, Side};
use tauri::{AppHandle, Emitter, Manager, State};

pub use execution_engine::RiskLimits;

use batch::{Batch, BatchDto};
use dto::{
    AccountDto, BarDto, BoardDto, ConnectionDto, InstrumentDto, MarketDto, OrderDto, QuoteDto,
    SessionDto, Snapshot,
};

const EVENT_NAME: &str = "bus-batch";
/// How often conflated changes go to the front end: fast enough that an
/// order's fill feels immediate, slow enough that a burst costs one message.
const FLUSH_INTERVAL: Duration = Duration::from_millis(50);
const DEFAULT_BAR_LIMIT: usize = 500;

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
    let (orders, orders_connection, account, can_amend) = {
        let mut exec = lock(&shell.exec)?;
        (
            exec.orders().map(OrderDto::from).collect(),
            ConnectionDto::of(Connection::Orders, exec.connection()),
            AccountDto::from(exec.account()),
            exec.can_amend(),
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
    })
}

/// Takes everything pending as the next numbered batch; `None` if nothing
/// changed since the last one.
fn take_batch(shell: &Shell) -> Result<Option<BatchDto>, String> {
    let batch = mem::take(&mut *lock(&shell.pending)?);
    if batch.is_empty() {
        return Ok(None);
    }
    let seq = shell.flushed.fetch_add(1, Ordering::SeqCst) + 1;
    Ok(Some(batch.into_dto(seq)))
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

#[tauri::command]
fn cancel_order(shell: State<'_, Shell>, id: u64) -> Result<OrderDto, String> {
    cancel(&shell, id)
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
            let board = {
                let mut data = lock(&shell.data)?;
                data.on_trade(trade)
                    .then(|| BoardDto::of(&data, &trade.instrument))
            };
            if let Some(board) = board {
                lock(&shell.pending)?.board(board);
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
                Ok(order) => lock(&shell.pending)?.order(OrderDto::from(&order)),
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
        .plugin(tauri_plugin_window_state::Builder::default().build())
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
            place_order,
            cancel_order,
            amend_order,
            workspace::settings_get,
            workspace::settings_set,
            workspace::settings_remove,
            workspace::open_window
        ])
        .build(tauri::generate_context!());

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
    fn batches_are_numbered_and_a_snapshot_carries_the_last_number() {
        let shell = shell();
        shell.pending.lock().unwrap().account(AccountDto {
            cash: "1".into(),
            positions: vec![],
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
