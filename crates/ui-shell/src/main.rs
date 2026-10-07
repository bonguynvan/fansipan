//! Desktop shell. Wires the bus to the engines and the simulated adapter, and
//! exposes a small command surface to the web front end.
//!
//! This is the one place that names a concrete adapter. Swapping the sandbox
//! for a real venue means changing `connect_engines` and nothing else.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod dto;

use std::process::ExitCode;
use std::sync::mpsc::Receiver;
use std::sync::{Mutex, MutexGuard};
use std::thread;

use adapter_api::AdapterError;
use bus::{Bus, Event};
use data_engine::DataEngine;
use execution_engine::{ExecutionEngine, RiskLimits};
use model::{AccountState, ClientOrderId, Decimal, InstrumentId, NewOrder, Side};
use sandbox::{SandboxDataClient, SandboxExecutionClient, SimVenue};
use tauri::{AppHandle, Emitter, Manager, State};

use dto::{AccountDto, OrderDto, QuoteDto, Snapshot, TickDto};

const VENUE: &str = "sim";
const STARTING_CASH: Decimal = Decimal::from_parts(100_000, 0, 0, false, 0);
const MAX_ORDER_QUANTITY: Decimal = Decimal::from_parts(1_000, 0, 0, false, 0);
const MAX_ORDER_NOTIONAL: Decimal = Decimal::from_parts(250_000, 0, 0, false, 0);
const EVENT_NAME: &str = "bus-event";

/// Symbol and opening mid in cents.
const LISTINGS: [(&str, i64); 5] = [
    ("AAPL", 19_000),
    ("MSFT", 41_000),
    ("NVDA", 88_000),
    ("TSLA", 24_000),
    ("VNM", 6_250),
];

pub struct Shell {
    data: Mutex<DataEngine>,
    exec: Mutex<ExecutionEngine>,
    account: Mutex<AccountState>,
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

#[tauri::command]
fn snapshot(shell: State<'_, Shell>) -> Result<Snapshot, String> {
    Ok(Snapshot {
        quotes: lock(&shell.data)?.quotes().map(QuoteDto::from).collect(),
        orders: lock(&shell.exec)?.orders().map(OrderDto::from).collect(),
        account: AccountDto::from(&*lock(&shell.account)?),
    })
}

/// Returns the order as recorded, including a risk denial: a denied order is
/// a result to show in the blotter, not a failed command.
#[tauri::command]
fn place_order(
    app: AppHandle,
    shell: State<'_, Shell>,
    symbol: String,
    side: String,
    quantity: String,
    limit_price: Option<String>,
) -> Result<OrderDto, String> {
    let request = new_order(&symbol, &side, &quantity, limit_price.as_deref())?;
    let order = OrderDto::from(&lock(&shell.exec)?.submit(request));
    let _ = app.emit(EVENT_NAME, TickDto::Order(order.clone()));
    Ok(order)
}

#[tauri::command]
fn cancel_order(app: AppHandle, shell: State<'_, Shell>, id: u64) -> Result<OrderDto, String> {
    let order = lock(&shell.exec)?
        .cancel(ClientOrderId(id))
        .map(|o| OrderDto::from(&o))
        .map_err(|e| e.to_string())?;
    let _ = app.emit(EVENT_NAME, TickDto::Order(order.clone()));
    Ok(order)
}

/// Routes each bus event to the engine that owns it, then tells the front
/// end what changed.
fn route(shell: &Shell, event: &Event) -> Result<Option<TickDto>, String> {
    Ok(match event {
        Event::Quote(quote) => lock(&shell.data)?
            .on_quote(quote)
            .then(|| TickDto::Quote(QuoteDto::from(quote))),
        Event::Execution(report) => match lock(&shell.exec)?.apply(report) {
            Ok(order) => Some(TickDto::Order(OrderDto::from(&order))),
            Err(e) => {
                eprintln!(
                    "execution report for order {} ignored: {e}",
                    report.order_id
                );
                None
            }
        },
        Event::AccountUpdate(account) => {
            *lock(&shell.account)? = account.clone();
            Some(TickDto::Account(AccountDto::from(account)))
        }
        _ => None,
    })
}

fn pump(app: AppHandle, events: Receiver<Event>) {
    thread::spawn(move || {
        let shell = app.state::<Shell>();
        for event in events {
            match route(&shell, &event) {
                Ok(Some(tick)) => {
                    let _ = app.emit(EVENT_NAME, tick);
                }
                Ok(None) => {}
                Err(e) => {
                    eprintln!("event pump stopped: {e}");
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
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            snapshot,
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
    fn a_report_for_an_unknown_order_produces_no_tick() {
        let bus = Bus::new();
        let shell = connect_engines(&bus).unwrap();
        let report = Event::Execution(model::ExecutionReport {
            order_id: ClientOrderId(404),
            event: model::OrderEvent::Cancelled,
            ts_event: model::UnixNanos(0),
        });

        assert!(matches!(route(&shell, &report), Ok(None)));
    }
}
