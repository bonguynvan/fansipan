//! Desktop shell. Wires the bus to the engines and the simulated adapter, and
//! exposes a small command surface to the web front end.
//!
//! This is the one place that names a concrete adapter. Swapping the sandbox
//! for a real venue means changing `connect_engines` and nothing else.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod batch;
mod dto;

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
use sandbox::{SandboxDataClient, SandboxExecutionClient, SimVenue};
use tauri::{AppHandle, Emitter, Manager, State};

use batch::{Batch, BatchDto};
use dto::{AccountDto, BarDto, OrderDto, QuoteDto, Snapshot};

const VENUE: &str = "sim";
const STARTING_CASH: Decimal = Decimal::from_parts(100_000, 0, 0, false, 0);
const MAX_ORDER_QUANTITY: Decimal = Decimal::from_parts(1_000, 0, 0, false, 0);
const MAX_ORDER_NOTIONAL: Decimal = Decimal::from_parts(250_000, 0, 0, false, 0);
const EVENT_NAME: &str = "bus-batch";
/// How often conflated changes go to the front end: fast enough that an
/// order's fill feels immediate, slow enough that a burst costs one message.
const FLUSH_INTERVAL: Duration = Duration::from_millis(50);
const DEFAULT_BAR_LIMIT: usize = 500;

/// Symbol and opening mid in cents.
const LISTINGS: [(&str, i64); 5] = [
    ("AAPL", 19_000),
    ("MSFT", 41_000),
    ("NVDA", 88_000),
    ("TSLA", 24_000),
    ("VNM", 6_250),
];

/// Lock order: `exec` before `pending`. Every change to an order is queued
/// in `pending` while `exec` is still held, so the batch receives order
/// states in the order the engine produced them, whichever thread made the
/// change. Nothing takes `pending` and then another lock.
pub struct Shell {
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
    Ok(Snapshot {
        seq,
        quotes: lock(&shell.data)?.quotes().map(QuoteDto::from).collect(),
        orders: lock(&shell.exec)?.orders().map(OrderDto::from).collect(),
        account: AccountDto::from(&*lock(&shell.account)?),
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
                    (QuoteDto::from(quote), bar)
                })
            };
            if let Some((quote, bar)) = update {
                let mut pending = lock(&shell.pending)?;
                pending.quote(quote);
                if let Some(bar) = bar {
                    pending.bar(bar);
                }
            }
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

fn connect_engines(bus: &Bus) -> Result<Shell, AdapterError> {
    let venue = SimVenue::new(STARTING_CASH);
    for (symbol, cents) in LISTINGS {
        venue.list(instrument(symbol), Decimal::new(cents, 2));
    }

    let mut data = DataEngine::new(Box::new(SandboxDataClient::new(venue.clone())));
    data.connect(bus.handle())?;
    for (symbol, _) in LISTINGS {
        data.subscribe_quotes(&instrument(symbol))?;
    }

    let mut exec = ExecutionEngine::new(
        Box::new(SandboxExecutionClient::new(venue)),
        RiskLimits {
            max_order_quantity: MAX_ORDER_QUANTITY,
            max_order_notional: MAX_ORDER_NOTIONAL,
        },
    );
    exec.connect(bus.handle())?;

    Ok(Shell {
        data: Mutex::new(data),
        exec: Mutex::new(exec),
        account: Mutex::new(AccountState::default()),
        pending: Mutex::new(Batch::default()),
        flushed: AtomicU64::new(0),
    })
}

fn main() -> ExitCode {
    let bus = Bus::new();
    let events = bus.subscribe();
    let shell = match connect_engines(&bus) {
        Ok(shell) => shell,
        Err(e) => {
            eprintln!("could not connect to the venue: {e}");
            return ExitCode::FAILURE;
        }
    };

    let result = tauri::Builder::default()
        .setup(move |app| {
            app.manage(shell);
            pump(app.handle().clone(), events);
            flush(app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            snapshot,
            bars,
            place_order,
            cancel_order
        ])
        .run(tauri::generate_context!());

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("Fansipan stopped: {e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use model::{Quote, UnixNanos};

    fn shell() -> Shell {
        connect_engines(&Bus::new()).unwrap()
    }

    fn take_pending(shell: &Shell) -> BatchDto {
        take_batch(shell).unwrap().unwrap_or(BatchDto {
            seq: 0,
            quotes: vec![],
            orders: vec![],
            account: None,
            bars: vec![],
        })
    }

    #[test]
    fn a_market_order_reaches_the_batch_as_filled() {
        let bus = Bus::new();
        let events = bus.subscribe();
        let shell = connect_engines(&bus).unwrap();

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
    }
}
