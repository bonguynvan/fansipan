//! Desktop shell. Wires the bus to the engines and the simulated adapter, and
//! exposes a small command surface to the web front end.
//!
//! This is the one place that names a concrete adapter. Swapping the sandbox
//! for a real venue means changing `connect_engines` and nothing else.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod batch;
mod dto;
mod settings;
mod workspace;

use std::collections::BTreeSet;
use std::env;
use std::mem;
use std::process::ExitCode;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::{Mutex, MutexGuard};
use std::thread;
use std::time::Duration;

use adapter_api::AdapterError;
use bus::{Bus, Event};
use data_engine::DataEngine;
use execution_engine::{ExecutionEngine, RiskLimits};
use model::{AccountState, ClientOrderId, Decimal, InstrumentId, NewOrder, Side};
use sandbox::{MarketProfile, SandboxDataClient, SandboxExecutionClient};
use tauri::{AppHandle, Emitter, Manager, State};

use batch::{Batch, BatchDto};
use dto::{AccountDto, BarDto, BoardDto, InstrumentDto, MarketDto, OrderDto, QuoteDto, Snapshot};

const VENUE: &str = "sim";
/// Which simulated market to run; see `sandbox::profiles::PROFILE_IDS`.
const MARKET_ENV: &str = "FANSIPAN_SANDBOX_MARKET";
const DEFAULT_MARKET: &str = "vn";
const EVENT_NAME: &str = "bus-batch";
/// How often conflated changes go to the front end: fast enough that an
/// order's fill feels immediate, slow enough that a burst costs one message.
const FLUSH_INTERVAL: Duration = Duration::from_millis(50);
const DEFAULT_BAR_LIMIT: usize = 500;

/// Lock order: `exec` before `pending`. Every change to an order is queued
/// in `pending` while `exec` is still held, so the batch receives order
/// states in the order the engine produced them, whichever thread made the
/// change. Nothing takes `pending` and then another lock.
pub struct Shell {
    market: MarketDto,
    data: Mutex<DataEngine>,
    exec: Mutex<ExecutionEngine>,
    account: Mutex<AccountState>,
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

fn instrument(symbol: &str) -> InstrumentId {
    InstrumentId::new(VENUE, symbol)
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

fn new_order(
    symbol: &str,
    side: &str,
    quantity: &str,
    limit_price: Option<&str>,
) -> Result<NewOrder, String> {
    Ok(NewOrder {
        instrument: instrument(symbol),
        side: parse_side(side)?,
        quantity: parse_decimal("quantity", quantity)?,
        limit_price: limit_price
            .map(|raw| parse_decimal("limit price", raw))
            .transpose()?,
    })
}

/// Everything the front end needs to start, numbered against the batches.
fn snapshot_of(shell: &Shell) -> Result<Snapshot, String> {
    // Read before any state: every batch numbered up to `seq` was taken
    // from `pending` before the state below was read, so it holds nothing
    // newer than this snapshot and the front end can drop it.
    let seq = shell.flushed.load(Ordering::SeqCst);
    let (instruments, boards, quotes) = {
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
        )
    };
    let orders = lock(&shell.exec)?.orders().map(OrderDto::from).collect();
    let account = AccountDto::from(&*lock(&shell.account)?);
    Ok(Snapshot {
        seq,
        market: shell.market.clone(),
        instruments,
        boards,
        quotes,
        orders,
        account,
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
        .bars(&instrument(&symbol), limit)
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
) -> Result<OrderDto, String> {
    submit(
        &shell,
        new_order(&symbol, &side, &quantity, limit_price.as_deref())?,
    )
}

#[tauri::command]
fn cancel_order(shell: State<'_, Shell>, id: u64) -> Result<OrderDto, String> {
    cancel(&shell, id)
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
        Event::AccountUpdate(account) => {
            *lock(&shell.account)? = account.clone();
            lock(&shell.pending)?.account(AccountDto::from(account));
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

/// The simulated market named by `FANSIPAN_SANDBOX_MARKET`, or the default.
fn market_profile() -> Result<MarketProfile, String> {
    let id = env::var(MARKET_ENV).unwrap_or_else(|_| DEFAULT_MARKET.to_string());
    MarketProfile::by_id(id.trim(), VENUE).map_err(|e| format!("{MARKET_ENV}: {e}"))
}

fn connect_engines(bus: &Bus, profile: &MarketProfile) -> Result<Shell, AdapterError> {
    let venue = profile.venue();

    let mut data = DataEngine::new(Box::new(SandboxDataClient::new(venue.clone())));
    data.connect(bus.handle())?;
    for listing in &profile.listings {
        data.subscribe_quotes(&listing.info.id)?;
        data.subscribe_trades(&listing.info.id)?;
    }

    let mut exec = ExecutionEngine::new(
        Box::new(SandboxExecutionClient::new(venue)),
        RiskLimits {
            max_order_quantity: profile.max_order_quantity,
            max_order_notional: profile.max_order_notional,
        },
    );
    exec.connect(bus.handle())?;

    Ok(Shell {
        market: MarketDto {
            id: profile.id.to_string(),
            title: profile.title.to_string(),
            currency: profile.currency.to_string(),
        },
        data: Mutex::new(data),
        exec: Mutex::new(exec),
        account: Mutex::new(AccountState::default()),
        pending: Mutex::new(Batch::default()),
        flushed: AtomicU64::new(0),
    })
}

fn main() -> ExitCode {
    let profile = match market_profile() {
        Ok(profile) => profile,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };
    let bus = Bus::new();
    let events = bus.subscribe();
    let shell = match connect_engines(&bus, &profile) {
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

    fn profile(id: &str) -> MarketProfile {
        MarketProfile::by_id(id, VENUE).unwrap()
    }

    fn shell() -> Shell {
        connect_engines(&Bus::new(), &profile("us")).unwrap()
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
        })
    }

    #[test]
    fn a_market_order_reaches_the_batch_as_filled() {
        let bus = Bus::new();
        let events = bus.subscribe();
        let shell = connect_engines(&bus, &profile("us")).unwrap();

        let placed = submit(&shell, new_order("AAPL", "buy", "1", None).unwrap()).unwrap();
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
        let order = new_order("AAPL", "buy", " 10 ", Some("190.10")).unwrap();

        assert_eq!(order.quantity, Decimal::TEN);
        assert_eq!(order.limit_price, Some(Decimal::new(19_010, 2)));
        assert_eq!(order.instrument, instrument("AAPL"));
    }

    #[test]
    fn market_orders_have_no_limit_price() {
        let order = new_order("AAPL", "sell", "1", None).unwrap();

        assert_eq!(order.side, Side::Sell);
        assert_eq!(order.limit_price, None);
    }

    #[test]
    fn malformed_input_is_refused_with_the_field_named() {
        assert_eq!(
            new_order("AAPL", "buy", "ten", None).unwrap_err(),
            "quantity: \"ten\" is not a number"
        );
        assert_eq!(
            new_order("AAPL", "buy", "1", Some("")).unwrap_err(),
            "limit price: \"\" is not a number"
        );
        assert_eq!(
            new_order("AAPL", "hold", "1", None).unwrap_err(),
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
            instrument: instrument("AAPL"),
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
        let shell = connect_engines(&Bus::new(), &vn).unwrap();

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
        let shell = connect_engines(&Bus::new(), &vn).unwrap();
        announce(&shell, &vn);

        let odd_lot = submit(
            &shell,
            new_order("VNM", "buy", "150", Some("62500")).unwrap(),
        );
        let above_ceiling = submit(
            &shell,
            new_order("VNM", "buy", "100", Some("66900")).unwrap(),
        );

        assert_eq!(odd_lot.unwrap().status, "denied");
        assert_eq!(above_ceiling.unwrap().status, "denied");
    }

    #[test]
    fn a_trade_moves_the_board_change_against_the_reference() {
        let vn = profile("vn");
        let shell = connect_engines(&Bus::new(), &vn).unwrap();
        announce(&shell, &vn);
        take_pending(&shell);

        route(
            &shell,
            &Event::Trade(model::Trade {
                instrument: instrument("VNM"),
                price: Decimal::new(63_100, 0),
                quantity: Decimal::new(300, 0),
                side: Side::Buy,
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
    fn an_unknown_market_is_refused_naming_the_setting() {
        let err = MarketProfile::by_id("mars", VENUE)
            .map_err(|e| format!("{MARKET_ENV}: {e}"))
            .unwrap_err();

        assert!(err.starts_with("FANSIPAN_SANDBOX_MARKET"));
    }
}
