//! The roadmap's Phase 1 acceptance loop, headless: an order goes through
//! risk, the execution engine, the sandbox adapter, back over the bus as
//! execution reports, and the engine's order state and the venue's account
//! agree at the end. Same wiring as the desktop shell, minus the window.

use std::sync::mpsc::Receiver;

use bus::{Bus, Event};
use data_engine::DataEngine;
use execution_engine::{CancelError, ExecutionEngine, RiskLimits};
use model::{
    AccountState, ClientOrderId, Decimal, InstrumentId, NewOrder, Order, OrderStatus, Side,
    UnixNanos,
};
use sandbox::{SandboxDataClient, SandboxExecutionClient, SimVenue};

fn d(raw: &str) -> Decimal {
    raw.parse().expect("test decimal")
}

fn aaa() -> InstrumentId {
    InstrumentId::new("sim", "AAA")
}

struct Desk {
    bus: Bus,
    events: Receiver<Event>,
    venue: SimVenue,
    exec: ExecutionEngine,
    data: DataEngine,
    account: AccountState,
}

impl Desk {
    fn open() -> Self {
        let bus = Bus::new();
        let events = bus.subscribe();
        let venue = SimVenue::new(d("100000"));
        venue.list(aaa(), d("10"));

        let mut exec = ExecutionEngine::new(
            Box::new(SandboxExecutionClient::new(venue.clone())),
            RiskLimits {
                max_order_quantity: d("1000"),
                max_order_notional: d("50000"),
            },
        );
        exec.connect(bus.handle()).expect("exec connects");
        let mut data = DataEngine::new(Box::new(SandboxDataClient::new(venue.clone())));
        data.subscribe_quotes(&aaa()).expect("subscribes");

        let mut desk = Self {
            bus,
            events,
            venue,
            exec,
            data,
            account: AccountState::default(),
        };
        desk.pump();
        desk
    }

    /// What the shell's pump thread does: route every bus event to its owner.
    fn pump(&mut self) {
        for event in self.events.try_iter() {
            match event {
                Event::Execution(report) => {
                    let _ = self.exec.apply(&report);
                }
                Event::AccountUpdate(account) => self.account = account,
                Event::Quote(quote) => {
                    self.data.on_quote(&quote);
                }
                _ => {}
            }
        }
    }

    /// What the sandbox market thread does on a beat, with a chosen move.
    fn move_price(&mut self, delta: &str) {
        let bus = &self.bus;
        let quote = self
            .venue
            .step(&aaa(), d(delta), UnixNanos(1), &mut |e| bus.publish(e))
            .expect("listed");
        self.bus.publish(Event::Quote(quote));
    }

    fn buy(&mut self, quantity: &str, limit: Option<&str>) -> ClientOrderId {
        self.exec
            .submit(NewOrder {
                instrument: aaa(),
                side: Side::Buy,
                quantity: d(quantity),
                limit_price: limit.map(d),
            })
            .id
    }

    fn order(&self, id: ClientOrderId) -> Order {
        self.exec.order(id).cloned().expect("order exists")
    }
}

#[test]
fn market_buy_fills_and_the_account_follows() {
    let mut desk = Desk::open();

    let id = desk.buy("5", None);
    desk.pump();

    let order = desk.order(id);
    assert_eq!(order.status, OrderStatus::Filled);
    assert_eq!(order.avg_fill_price, Some(d("10.01")));
    assert!(order.venue_order_id.is_some());
    assert_eq!(desk.account.cash, d("100000") - d("50.05"));
    assert_eq!(desk.account.positions[0].quantity, d("5"));
}

#[test]
fn resting_limit_fills_in_parts_as_the_price_walks_through_it() {
    let mut desk = Desk::open();
    let id = desk.buy("250", Some("9.95"));
    desk.pump();
    assert_eq!(desk.order(id).status, OrderStatus::Accepted);

    desk.move_price("-0.06");
    desk.pump();
    assert_eq!(desk.order(id).status, OrderStatus::PartiallyFilled);
    assert_eq!(desk.order(id).filled_quantity, d("100"));

    desk.move_price("0");
    desk.move_price("0");
    desk.pump();
    let done = desk.order(id);
    assert_eq!(done.status, OrderStatus::Filled);
    assert_eq!(done.avg_fill_price, Some(d("9.95")));
    assert_eq!(desk.account.positions[0].quantity, d("250"));
}

#[test]
fn quotes_reach_the_data_engine_for_subscribed_instruments() {
    let mut desk = Desk::open();

    desk.move_price("0.05");
    desk.pump();

    assert_eq!(desk.data.latest(&aaa()).map(|q| q.mid()), Some(d("10.05")));
}

#[test]
fn cancel_round_trip_ends_cancelled() {
    let mut desk = Desk::open();
    let id = desk.buy("10", Some("9"));
    desk.pump();

    let pending = desk.exec.cancel(id).expect("cancel sent");
    desk.pump();

    assert_eq!(pending.status, OrderStatus::PendingCancel);
    assert_eq!(desk.order(id).status, OrderStatus::Cancelled);
    assert_eq!(
        desk.exec.cancel(id),
        Err(CancelError::NotOpen(OrderStatus::Cancelled))
    );
}

#[test]
fn a_cancel_that_loses_the_race_to_a_fill_ends_filled() {
    let mut desk = Desk::open();
    let id = desk.buy("50", Some("9.95"));
    desk.pump();

    desk.move_price("-0.06");
    let pending = desk.exec.cancel(id).expect("cancel sent");
    desk.pump();

    assert_eq!(pending.status, OrderStatus::PendingCancel);
    let order = desk.order(id);
    assert_eq!(order.status, OrderStatus::Filled);
    assert!(order.reason.is_some());
}

#[test]
fn risk_denial_never_reaches_the_venue() {
    let mut desk = Desk::open();

    let id = desk.buy("5000", None);
    desk.pump();

    assert_eq!(desk.order(id).status, OrderStatus::Denied);
    assert_eq!(desk.venue.account().cash, d("100000"));
    assert!(desk.venue.account().positions.is_empty());
}
