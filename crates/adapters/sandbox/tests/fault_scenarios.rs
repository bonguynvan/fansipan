//! Faults, replayed deterministically. Between the
//! sandbox venue and the execution engine runs a wire that loses the
//! venue's reports while the connection is down, delivers some twice, and
//! holds others back to deliver them out of order. A reconnect reads the
//! venue's orders back and publishes them as snapshots, as the contract asks
//! of an adapter; what was in flight when the connection dropped is lost
//! with it, so no stream report older than a read-back arrives after it.
//!
//! However the reports fare, once the last read-back is in, the engine
//! agrees with the venue on every order, holds the venue's account, and the
//! account's cash and holdings reconcile with the engine's fills.
//!
//! Requests always reach the venue: these are faults of the report stream.
//! Scenarios come from a fixed seed, so every run replays the same faults;
//! set PROPTEST_RNG_SEED to explore others.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use bus::{BusHandle, Event};
use execution_engine::{ExecutionEngine, RiskLimits};
use fansipan_sandbox::{Listing, SandboxExecutionClient, SimVenue};
use model::{
    ClientOrderId, Decimal, ExecutionReport, InstrumentId, NewOrder, Order, OrderEvent,
    OrderStatus, Side, UnixNanos, VenueOrder, VenueOrderState,
};
use proptest::collection::vec;
use proptest::prelude::*;
use proptest::test_runner::RngSeed;

const SEED: u64 = 20_261_008;
const STARTING_CASH: i64 = 1_000_000;
/// Little enough cash that buys run out of it, so the venue refuses some.
const TIGHT_CASH: i64 = 8_000;
const MID: i64 = 1_000;
const NOW: UnixNanos = UnixNanos(1);

fn config() -> ProptestConfig {
    let config = ProptestConfig::default();
    if std::env::var_os("PROPTEST_RNG_SEED").is_some() {
        return config;
    }
    ProptestConfig {
        rng_seed: RngSeed::Fixed(SEED),
        ..config
    }
}

fn aaa() -> InstrumentId {
    InstrumentId::new("sim", "AAA")
}

/// A price `ticks` hundredths from 10.00.
fn price(ticks: i32) -> Decimal {
    Decimal::new(MID + i64::from(ticks), 2)
}

/// What the wire does with a report the venue sent.
#[derive(Debug, Clone, Copy)]
enum Fate {
    Deliver,
    Twice,
    Hold,
}

/// One thing that happens: a trader's action, the market moving, or the
/// wire doing something.
#[derive(Debug, Clone)]
enum Step {
    Buy {
        quantity: u32,
        ticks: i32,
    },
    Sell {
        quantity: u32,
        ticks: i32,
    },
    MarketBuy {
        quantity: u32,
    },
    Cancel {
        pick: usize,
    },
    Amend {
        pick: usize,
        quantity: u32,
        ticks: i32,
    },
    Move {
        ticks: i32,
    },
    /// The wire delivers what the venue sent, each report to its fate.
    Deliver,
    /// Held reports arrive, latest first.
    Release,
    /// The connection drops: whatever is in flight is lost with it.
    Drop,
    /// The adapter reconnects and reads the venue's orders back.
    Reconnect,
}

/// Sizes below the book's displayed size fill at once; larger ones fill in
/// parts over several moves, where a duplicated or missed fill matters.
fn size() -> impl Strategy<Value = u32> {
    prop_oneof![1u32..50, 100u32..400]
}

fn step() -> impl Strategy<Value = Step> {
    prop_oneof![
        3 => (size(), -20i32..20).prop_map(|(quantity, ticks)| Step::Buy { quantity, ticks }),
        3 => (size(), -20i32..20).prop_map(|(quantity, ticks)| Step::Sell { quantity, ticks }),
        1 => size().prop_map(|quantity| Step::MarketBuy { quantity }),
        2 => (0usize..8).prop_map(|pick| Step::Cancel { pick }),
        2 => (0usize..8, size(), -20i32..20)
            .prop_map(|(pick, quantity, ticks)| Step::Amend { pick, quantity, ticks }),
        4 => (-6i32..=6).prop_map(|ticks| Step::Move { ticks }),
        4 => Just(Step::Deliver),
        1 => Just(Step::Release),
        1 => Just(Step::Drop),
        1 => Just(Step::Reconnect),
    ]
}

fn fate() -> impl Strategy<Value = Fate> {
    prop_oneof![6 => Just(Fate::Deliver), 2 => Just(Fate::Twice), 2 => Just(Fate::Hold)]
}

/// The venue's end of the wire: it publishes here, the wire decides.
struct WireEnd(Arc<Mutex<VecDeque<Event>>>);

impl BusHandle for WireEnd {
    fn publish(&self, event: Event) {
        if let Ok(mut sent) = self.0.lock() {
            sent.push_back(event);
        }
    }
}

struct Desk {
    venue: SimVenue,
    starting_cash: Decimal,
    exec: ExecutionEngine,
    sent: Arc<Mutex<VecDeque<Event>>>,
    held: Vec<Event>,
    connected: bool,
    fates: Vec<Fate>,
    next_fate: usize,
}

impl Desk {
    /// A desk on the sandbox. A tight one has little cash and a market that
    /// forbids short selling, so the venue refuses buys it cannot pay for
    /// and sells beyond the holding; otherwise it refuses nothing.
    fn open(fates: Vec<Fate>, tight: bool) -> Self {
        let starting_cash = Decimal::from(if tight { TIGHT_CASH } else { STARTING_CASH });
        let venue = SimVenue::new(starting_cash);
        let mut listing = Listing::unrestricted(aaa(), price(0)).expect("a listing");
        listing.info = listing.info.with_short_selling(!tight);
        venue.list_with(listing);
        let mut exec = ExecutionEngine::new(
            Box::new(SandboxExecutionClient::new(venue.clone())),
            RiskLimits {
                max_order_quantity: Decimal::from(1_000),
                max_order_notional: Decimal::from(1_000_000),
            },
        );
        let sent = Arc::new(Mutex::new(VecDeque::new()));
        exec.connect(Box::new(WireEnd(sent.clone())))
            .expect("the engine connects");
        Self {
            venue,
            starting_cash,
            exec,
            sent,
            held: Vec::new(),
            connected: true,
            fates,
            next_fate: 0,
        }
    }

    /// What the shell's pump does with a report that arrives.
    fn route(&mut self, event: Event) {
        match event {
            Event::Execution(report) => {
                let _ = self.exec.apply(&report);
            }
            Event::AccountUpdate(account) => self.exec.set_account(account),
            _ => {}
        }
    }

    fn take_sent(&mut self) -> Vec<Event> {
        self.sent
            .lock()
            .map(|mut sent| sent.drain(..).collect())
            .unwrap_or_default()
    }

    fn next_fate(&mut self) -> Fate {
        let fate = self.fates[self.next_fate % self.fates.len()];
        self.next_fate += 1;
        fate
    }

    /// Open orders in the engine's view, oldest first: what a trader picks from.
    fn open_orders(&self) -> Vec<Order> {
        self.exec
            .orders()
            .filter(|o| o.status.is_open())
            .cloned()
            .collect()
    }

    fn pick(&self, pick: usize) -> Option<ClientOrderId> {
        let open = self.open_orders();
        (!open.is_empty()).then(|| open[pick % open.len()].id)
    }

    fn run(&mut self, step: &Step) {
        match *step {
            Step::Buy { quantity, ticks } => {
                let _ = self.exec.submit(NewOrder::limit(
                    aaa(),
                    Side::Buy,
                    Decimal::from(quantity),
                    price(ticks),
                ));
            }
            Step::Sell { quantity, ticks } => {
                let _ = self.exec.submit(NewOrder::limit(
                    aaa(),
                    Side::Sell,
                    Decimal::from(quantity),
                    price(ticks),
                ));
            }
            Step::MarketBuy { quantity } => {
                let _ =
                    self.exec
                        .submit(NewOrder::market(aaa(), Side::Buy, Decimal::from(quantity)));
            }
            Step::Cancel { pick } => {
                if let Some(id) = self.pick(pick) {
                    let _ = self.exec.cancel(id);
                }
            }
            Step::Amend {
                pick,
                quantity,
                ticks,
            } => {
                if let Some(id) = self.pick(pick) {
                    let _ = self.exec.amend(id, Decimal::from(quantity), price(ticks));
                }
            }
            Step::Move { ticks } => {
                let sent = self.sent.clone();
                let mut sink = |event| {
                    if let Ok(mut queue) = sent.lock() {
                        queue.push_back(event);
                    }
                };
                self.venue
                    .step(&aaa(), Decimal::new(i64::from(ticks), 2), NOW, &mut sink);
            }
            Step::Deliver => self.deliver(),
            Step::Release => {
                let held: Vec<Event> = self.held.drain(..).rev().collect();
                for event in held {
                    self.route(event);
                }
            }
            Step::Drop => {
                self.connected = false;
                self.held.clear();
                self.take_sent();
            }
            Step::Reconnect => self.reconnect(),
        }
    }

    fn deliver(&mut self) {
        for event in self.take_sent() {
            if !self.connected {
                continue;
            }
            match self.next_fate() {
                Fate::Deliver => self.route(event),
                Fate::Twice => {
                    self.route(event.clone());
                    self.route(event);
                }
                Fate::Hold => self.held.push(event),
            }
        }
    }

    /// What an adapter does on reconnecting: a new connection, so whatever
    /// the old one had in flight is gone, then the session's orders and the
    /// account read back. That keeps the contract's rule for adapters: no stream
    /// report older than a read-back arrives after it, or a fill the
    /// read-back already counted would count again.
    fn reconnect(&mut self) {
        self.take_sent();
        self.held.clear();
        self.connected = true;
        for (order_id, view) in self.venue.order_views() {
            self.route(Event::Execution(ExecutionReport {
                order_id,
                event: OrderEvent::Snapshot(view),
                ts_event: NOW,
            }));
        }
        self.route(Event::AccountUpdate(self.venue.account()));
    }

    /// The end of the session: the wire comes back, everything still on it
    /// arrives, and a last read-back settles what the stream left out.
    fn settle(&mut self) {
        if self.connected {
            let held = std::mem::take(&mut self.held);
            for event in held {
                self.route(event);
            }
            for event in self.take_sent() {
                self.route(event);
            }
        }
        self.reconnect();
    }
}

/// Whether the engine knows no more fills than the venue made: duplicates
/// and replays never count twice, at any point of the session.
fn never_ahead_of_the_venue(desk: &Desk) -> Result<(), String> {
    for (id, view) in desk.venue.order_views() {
        let filled = desk
            .exec
            .order(id)
            .map_or(Decimal::ZERO, |o| o.filled_quantity);
        if filled > view.filled_quantity {
            return Err(format!(
                "order {id:?}: {filled} filled, the venue made {}",
                view.filled_quantity
            ));
        }
    }
    Ok(())
}

/// Whether the engine's order says what the venue's view says.
fn agrees(order: &Order, view: &VenueOrder) -> Result<(), String> {
    if order.filled_quantity != view.filled_quantity {
        return Err(format!(
            "filled {} but the venue says {}",
            order.filled_quantity, view.filled_quantity
        ));
    }
    let status_agrees = match &view.state {
        VenueOrderState::Working => order.status.is_open(),
        VenueOrderState::Filled => order.status == OrderStatus::Filled,
        VenueOrderState::Cancelled => order.status == OrderStatus::Cancelled,
        // A rejection after fills ends as a cancel.
        VenueOrderState::Rejected { .. } => {
            order.status == OrderStatus::Rejected
                || (order.status == OrderStatus::Cancelled && !order.filled_quantity.is_zero())
        }
        _ => false,
    };
    if !status_agrees {
        return Err(format!(
            "{:?} but the venue says {:?}",
            order.status, view.state
        ));
    }
    match (order.avg_fill_price, view.avg_fill_price) {
        (Some(ours), Some(venues)) if (ours - venues).abs() > Decimal::new(1, 12) => {
            return Err(format!("average {ours} but the venue says {venues}"));
        }
        (Some(_), Some(_)) | (None, None) => {}
        (ours, venues) => return Err(format!("average {ours:?} but the venue says {venues:?}")),
    }
    // The terms too: an amendment whose answer was missed is settled by the
    // read-back.
    if view.quantity.is_some_and(|q| q != order.quantity) || view.limit_price != order.limit_price {
        return Err(format!(
            "terms {} at {:?} but the venue says {:?} at {:?}",
            order.quantity, order.limit_price, view.quantity, view.limit_price
        ));
    }
    Ok(())
}

proptest! {
    #![proptest_config(config())]

    #[test]
    fn the_engine_ends_in_the_venues_state_whatever_the_wire_did(
        steps in vec(step(), 1..60),
        fates in vec(fate(), 1..20),
        tight in any::<bool>(),
    ) {
        let mut desk = Desk::open(fates, tight);
        for step in &steps {
            desk.run(step);
            let ahead = never_ahead_of_the_venue(&desk);
            prop_assert!(ahead.is_ok(), "after {step:?}: {}", ahead.err().unwrap_or_default());
        }
        desk.settle();

        // Every order agrees with the venue's view of it.
        for (id, view) in desk.venue.order_views() {
            let order = desk.exec.order(id);
            prop_assert!(order.is_some(), "the venue has order {id:?} the engine does not");
            if let Some(order) = order {
                let verdict = agrees(order, &view);
                prop_assert!(verdict.is_ok(), "order {id:?}: {}", verdict.err().unwrap_or_default());
            }
        }

        // The engine holds the venue's account, and it reconciles with the
        // engine's own fills: cash moved by what traded, holdings by what
        // was bought less what was sold.
        let account = desk.venue.account();
        prop_assert_eq!(desk.exec.account(), &account);
        let mut cash = desk.starting_cash;
        let mut held = Decimal::ZERO;
        for order in desk.exec.orders() {
            let traded = order.avg_fill_price.unwrap_or_default() * order.filled_quantity;
            match order.side {
                Side::Buy => {
                    cash -= traded;
                    held += order.filled_quantity;
                }
                Side::Sell => {
                    cash += traded;
                    held -= order.filled_quantity;
                }
            }
        }
        prop_assert!((cash - account.cash).abs() <= Decimal::new(1, 9), "cash {} from fills, {} at the venue", cash, account.cash);
        let position = account.position(&aaa()).map_or(Decimal::ZERO, |p| p.quantity);
        prop_assert_eq!(held, position);
    }
}

/// The story the generated scenarios tell many times over, once by hand: a
/// fill that happens while the connection is down never reaches the engine
/// on the stream, and the read-back at reconnect catches the order up.
#[test]
fn a_fill_lost_in_an_outage_is_caught_up_at_reconnect() {
    let mut desk = Desk::open(vec![Fate::Deliver], false);
    desk.run(&Step::Buy {
        quantity: 10,
        ticks: -5,
    });
    desk.run(&Step::Deliver);
    let resting = desk.exec.order(ClientOrderId(1)).cloned();

    desk.run(&Step::Drop);
    // The market falls through the buy's limit while the wire is down.
    desk.run(&Step::Move { ticks: -10 });
    desk.run(&Step::Deliver);
    let after_the_outage = desk.exec.order(ClientOrderId(1)).cloned();
    desk.run(&Step::Reconnect);
    let caught_up = desk.exec.order(ClientOrderId(1)).cloned();

    assert_eq!(resting.map(|o| o.status), Some(OrderStatus::Accepted));
    assert_eq!(
        after_the_outage.map(|o| o.filled_quantity),
        Some(Decimal::ZERO)
    );
    let caught_up = caught_up.expect("the engine has the order");
    assert_eq!(caught_up.status, OrderStatus::Filled);
    assert_eq!(caught_up.filled_quantity, Decimal::from(10));
    assert_eq!(caught_up.avg_fill_price, Some(price(-5)));
    assert_eq!(desk.exec.account(), &desk.venue.account());
}
