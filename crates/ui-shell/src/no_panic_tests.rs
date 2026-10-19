//! No panic on input from outside (roadmap Phase 7). Core parses no wire
//! protocol, but it reads what it cannot trust: what an adapter publishes
//! (quotes, prints, depth, rules, bands, order reports and read-backs, the
//! account, sessions, connections), whatever their values, and what a
//! trader types. Whatever arrives, the shell answers with a value or a
//! typed error. A panic inside an engine's lock would poison it and take
//! the terminal down over one bad message.
//!
//! The seed is fixed, so every run explores the same cases (CLAUDE.md:
//! tests are deterministic). Set PROPTEST_RNG_SEED to explore others. The
//! generators are boxed: nested as deep as these are, their unboxed types
//! overflow a debug build's test-thread stack.

use std::sync::mpsc::Receiver;

use model::{
    AccountState, BookLevel, ClientOrderId, Connection, ConnectionState, ConnectionStatus, Decimal,
    Depth, ExecutionReport, ExternalOrder, FillId, InstrumentId, InstrumentInfo, NamedOrderType,
    OrderEvent, OrderType, Position, PriceBand, Quote, SessionPhase, SessionStatus, Side,
    TickTable, Trade, UnixNanos, VenueOrder, VenueOrderId, VenueOrderState,
};
use proptest::collection::vec;
use proptest::prelude::*;
use proptest::sample::select;
use proptest::test_runner::RngSeed;
use sandbox::{MarketProfile, SandboxDataClient, SandboxExecutionClient};

use super::*;

const VENUE: &str = "sim";
const SEED: u64 = 20_261_008;

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

/// The Vietnamese sandbox, wired as `src/main.rs` wires it, with the bus
/// the pump would read: the venue's own answers to orders come back on it.
fn shell() -> (Shell, Receiver<Event>) {
    let profile = MarketProfile::by_id("vn", VENUE).expect("the vn profile");
    let sim = profile.venue();
    let venue = Venue::new(
        VENUE,
        Market::new(profile.id, profile.title, profile.currency),
        Box::new(SandboxDataClient::new(sim.clone())),
        Box::new(SandboxExecutionClient::new(sim)),
        RiskLimits {
            max_order_quantity: profile.max_order_quantity,
            max_order_notional: profile.max_order_notional,
        },
    )
    .subscribe(profile.listings.iter().map(|l| l.info.id.clone()));
    let bus = Bus::new();
    let events = bus.subscribe();
    let shell = connect_engines(&bus, venue).expect("the shell connects");
    (shell, events)
}

/// Any decimal an adapter could send: ordinary prices and sizes, zero, the
/// extremes, the smallest step, and any bit pattern at any scale.
fn decimal() -> BoxedStrategy<Decimal> {
    prop_oneof![
        4 => (-1_000_000i64..1_000_000, 0u32..6).prop_map(|(m, scale)| Decimal::new(m, scale)),
        1 => Just(Decimal::ZERO),
        1 => Just(Decimal::MAX),
        1 => Just(Decimal::MIN),
        1 => Just(Decimal::new(1, 28)),
        2 => (any::<u32>(), any::<u32>(), any::<u32>(), any::<bool>(), 0u32..=28)
            .prop_map(|(lo, mid, hi, negative, scale)| Decimal::from_parts(lo, mid, hi, negative, scale)),
    ].boxed()
}

/// A listed symbol most of the time, or one the venue never listed.
fn symbol() -> BoxedStrategy<String> {
    prop_oneof![4 => Just("VNM".to_string()), 1 => Just("ZZZ".to_string())].boxed()
}

fn instrument() -> BoxedStrategy<InstrumentId> {
    symbol().prop_map(|s| InstrumentId::new(VENUE, s)).boxed()
}

/// A tick ladder an adapter could publish: valid ones only, since
/// `TickTable::new` refuses the rest, but with any ticks and boundaries.
fn ticks() -> BoxedStrategy<TickTable> {
    vec((decimal(), decimal()), 1..4)
        .prop_filter_map("a ladder TickTable::new accepts", |steps| {
            let mut steps: Vec<(Decimal, Decimal)> = steps
                .into_iter()
                .map(|(from, tick)| (from.abs(), tick.abs()))
                .collect();
            steps[0].0 = Decimal::ZERO;
            steps.sort_by_key(|step| step.0);
            steps.dedup_by(|a, b| a.0 == b.0);
            TickTable::new(steps).ok()
        })
        .boxed()
}

/// Anything a venue could say about an order: what it did, or its view
/// after a reconnect, with any quantities and prices.
fn order_event() -> BoxedStrategy<OrderEvent> {
    let state = prop_oneof![
        Just(VenueOrderState::Working),
        Just(VenueOrderState::Filled),
        Just(VenueOrderState::Cancelled),
        Just(VenueOrderState::Rejected {
            reason: "refused".into()
        }),
    ];
    prop_oneof![
        Just(OrderEvent::Accepted {
            venue_order_id: VenueOrderId("V1".into())
        }),
        Just(OrderEvent::Rejected {
            reason: "refused".into()
        }),
        (0u8..4, decimal(), decimal()).prop_map(|(f, quantity, price)| OrderEvent::Filled {
            fill_id: FillId(format!("F{f}")),
            quantity,
            price,
        }),
        Just(OrderEvent::Cancelled),
        Just(OrderEvent::CancelRejected {
            reason: "too late".into()
        }),
        (decimal(), optional(decimal())).prop_map(|(quantity, limit_price)| OrderEvent::Amended {
            quantity,
            limit_price
        }),
        Just(OrderEvent::AmendRejected {
            reason: "refused".into()
        }),
        (
            state,
            decimal(),
            optional(decimal()),
            optional(decimal()),
            optional(decimal())
        )
            .prop_map(|(state, filled, average, quantity, limit)| {
                let view = VenueOrder::new(state, filled, average);
                let view = match quantity {
                    Some(quantity) => view.with_terms(quantity, limit),
                    None => view,
                };
                OrderEvent::Snapshot(view)
            },),
    ]
    .boxed()
}

/// A market's phase of the day, as any adapter could report it.
fn session() -> BoxedStrategy<SessionStatus> {
    let phase = prop_oneof![
        Just(SessionPhase::PreOpen),
        Just(SessionPhase::OpeningAuction),
        Just(SessionPhase::Continuous),
        Just(SessionPhase::Break),
        Just(SessionPhase::ClosingAuction),
        Just(SessionPhase::AfterClose),
        Just(SessionPhase::Closed),
    ];
    let market = select(vec!["HOSE", "HNX", "ZZZ", ""]);
    (market, phase, any::<bool>())
        .prop_map(|(market, phase, limit_only)| {
            let types = if limit_only {
                vec![NamedOrderType::new(OrderType::LIMIT, "LO")]
            } else {
                Vec::new()
            };
            SessionStatus::new(market, phase, UnixNanos(1)).with_order_types(types)
        })
        .boxed()
}

/// An account as any adapter could report it: any cash, buying power,
/// holdings, and settled shares.
fn account() -> BoxedStrategy<AccountState> {
    let position = (instrument(), decimal(), decimal(), optional(decimal())).prop_map(
        |(id, quantity, average, settled)| {
            let position = Position::new(id, quantity, average);
            match settled {
                Some(settled) => position.with_settled(settled),
                None => position,
            }
        },
    );
    (decimal(), optional(decimal()), vec(position, 0..3))
        .prop_map(|(cash, buying_power, positions)| {
            let account = AccountState::new(cash, positions);
            match buying_power {
                Some(power) => account.with_buying_power(power),
                None => account,
            }
        })
        .boxed()
}

fn connection() -> BoxedStrategy<ConnectionStatus> {
    let state = prop_oneof![
        Just(ConnectionState::Up),
        Just(ConnectionState::Degraded),
        Just(ConnectionState::Reconnecting),
        Just(ConnectionState::Down),
    ];
    (any::<bool>(), state, any::<String>())
        .prop_map(|(orders, state, detail)| {
            let connection = if orders {
                Connection::Orders
            } else {
                Connection::MarketData
            };
            ConnectionStatus::new(connection, state, UnixNanos(1)).with_detail(detail)
        })
        .boxed()
}

fn optional(value: impl Strategy<Value = Decimal>) -> impl Strategy<Value = Option<Decimal>> {
    prop_oneof![1 => Just(None), 3 => value.prop_map(Some)]
}

/// Anything the adapter can tell core about a market.
fn adapter_event() -> BoxedStrategy<Event> {
    prop_oneof![
        (instrument(), decimal(), decimal(), decimal(), decimal()).prop_map(
            |(instrument, bid, ask, bid_size, ask_size)| {
                Event::Quote(Quote {
                    instrument,
                    bid,
                    ask,
                    bid_size,
                    ask_size,
                    ts_event: UnixNanos(1),
                })
            }
        ),
        (instrument(), decimal(), decimal(), any::<bool>()).prop_map(
            |(instrument, price, quantity, buy)| {
                Event::Trade(Trade {
                    instrument,
                    price,
                    quantity,
                    side: Some(if buy { Side::Buy } else { Side::Sell }),
                    ts_event: UnixNanos(1),
                })
            }
        ),
        (
            instrument(),
            vec((decimal(), decimal()), 0..4),
            vec((decimal(), decimal()), 0..4)
        )
            .prop_map(|(instrument, bids, asks)| {
                let levels = |side: Vec<(Decimal, Decimal)>| {
                    side.into_iter()
                        .map(|(p, s)| BookLevel::new(p, s))
                        .collect()
                };
                Event::Depth(Depth::new(
                    instrument,
                    levels(bids),
                    levels(asks),
                    UnixNanos(1),
                ))
            }),
        (instrument(), decimal(), ticks(), any::<bool>()).prop_map(|(id, lot, ticks, short)| {
            Event::Instrument(InstrumentInfo::new(id, lot, ticks).with_short_selling(short))
        }),
        (
            instrument(),
            decimal(),
            optional(decimal()),
            optional(decimal())
        )
            .prop_map(|(id, reference, ceiling, floor)| {
                Event::PriceBand(PriceBand::new(id, reference, ceiling, floor, UnixNanos(1)))
            }),
        // Reports for orders that exist (ids from 1) and ones that never did.
        (0u64..6, order_event()).prop_map(|(id, event)| {
            Event::Execution(ExecutionReport {
                order_id: ClientOrderId(id),
                event,
                ts_event: UnixNanos(1),
            })
        }),
        account().prop_map(Event::AccountUpdate),
        session().prop_map(Event::Session),
        connection().prop_map(Event::Connection),
        // Orders placed elsewhere: a few venue ids, so some repeat.
        (
            0u8..3,
            instrument(),
            any::<bool>(),
            decimal(),
            optional(decimal()),
            prop_oneof![
                Just(VenueOrderState::Working),
                Just(VenueOrderState::Filled),
                Just(VenueOrderState::Cancelled),
            ],
            decimal(),
            optional(decimal()),
        )
            .prop_map(
                |(venue, id, buy, quantity, limit, state, filled, average)| {
                    Event::ExternalOrder(ExternalOrder::new(
                        VenueOrderId(format!("X-{venue}")),
                        id,
                        if buy { Side::Buy } else { Side::Sell },
                        quantity,
                        if limit.is_some() {
                            OrderType::LIMIT
                        } else {
                            OrderType::MARKET
                        },
                        limit,
                        VenueOrder::new(state, filled, average),
                    ))
                }
            ),
    ]
    .boxed()
}

/// Anything a trader could type into a numeric field, or send by hand.
fn typed() -> BoxedStrategy<String> {
    let pieces = select(vec!['0', '1', '9', '.', '-', '+', 'e', 'E', ' ', ',', '_']);
    prop_oneof![
        3 => decimal().prop_map(|d| d.to_string()),
        2 => vec(pieces, 0..40).prop_map(|chars| chars.into_iter().collect()),
        1 => any::<String>(),
        1 => Just("79228162514264337593543950336".to_string()),
        1 => Just("1e30".to_string()),
    ]
    .boxed()
}

fn word() -> BoxedStrategy<Option<String>> {
    let words = select(vec![
        "limit",
        "market",
        "marketToLimit",
        "day",
        "ioc",
        "fok",
        "atOpen",
        "atClose",
        "afterClose",
        "",
        "LIMIT",
        "buy",
    ]);
    prop_oneof![1 => Just(None), 3 => words.prop_map(|w| Some(w.to_string())), 1 => any::<String>().prop_map(Some)].boxed()
}

#[derive(Debug, Clone)]
enum Input {
    Adapter(Event),
    Place {
        symbol: String,
        side: String,
        quantity: String,
        price: Option<String>,
        kind: Option<String>,
        time_in_force: Option<String>,
    },
    Capacity {
        symbol: String,
        side: String,
        price: Option<String>,
        kind: Option<String>,
    },
    Cancel(u64),
    CancelAll(Option<String>),
    Amend {
        id: u64,
        quantity: String,
        price: String,
    },
    Conditional {
        symbol: String,
        side: String,
        quantity: String,
        trigger: String,
        level: String,
        percent: bool,
        limit_price: Option<String>,
    },
    CancelConditional(u64),
    Bracket {
        symbol: String,
        side: String,
        quantity: String,
        price: Option<String>,
        stop: Option<String>,
        trail: Option<String>,
        percent: bool,
        take_profit: Option<String>,
    },
    Sizing {
        symbol: String,
        side: String,
        entry: String,
        stop: String,
        risk: String,
        percent: bool,
    },
}

fn input() -> BoxedStrategy<Input> {
    let side = || {
        prop_oneof![
            Just("buy".to_string()),
            Just("sell".to_string()),
            any::<String>()
        ]
    };
    prop_oneof![
        6 => adapter_event().prop_map(Input::Adapter),
        3 => (symbol(), side(), typed(), prop::option::of(typed()), word(), word()).prop_map(
            |(symbol, side, quantity, price, kind, time_in_force)| Input::Place { symbol, side, quantity, price, kind, time_in_force },
        ),
        2 => (symbol(), side(), prop::option::of(typed()), word())
            .prop_map(|(symbol, side, price, kind)| Input::Capacity { symbol, side, price, kind }),
        1 => (0u64..6).prop_map(Input::Cancel),
        1 => prop::option::of(symbol()).prop_map(Input::CancelAll),
        1 => (0u64..6, typed(), typed()).prop_map(|(id, quantity, price)| Input::Amend { id, quantity, price }),
        2 => (
            symbol(),
            side(),
            typed(),
            prop_oneof![Just("at_or_below"), Just("at_or_above"), Just("trailing")],
            typed(),
            any::<bool>(),
            prop::option::of(typed()),
        )
            .prop_map(|(symbol, side, quantity, trigger, level, percent, limit_price)| Input::Conditional {
                symbol,
                side,
                quantity,
                trigger: trigger.to_string(),
                level,
                percent,
                limit_price,
            }),
        1 => (0u64..6).prop_map(Input::CancelConditional),
        2 => (
            symbol(),
            side(),
            typed(),
            prop::option::of(typed()),
            prop::option::of(typed()),
            prop::option::of(typed()),
            any::<bool>(),
            prop::option::of(typed()),
        )
            .prop_map(|(symbol, side, quantity, price, stop, trail, percent, take_profit)| Input::Bracket {
                symbol,
                side,
                quantity,
                price,
                stop,
                trail,
                percent,
                take_profit,
            }),
        1 => (symbol(), side(), typed(), typed(), typed(), any::<bool>())
            .prop_map(|(symbol, side, entry, stop, risk, percent)| Input::Sizing { symbol, side, entry, stop, risk, percent }),
    ].boxed()
}

/// Feeds one input to the shell the way its pump or a command would. The
/// result is a value or an error; only a panic fails the property.
fn feed(shell: &Shell, input: &Input) {
    match input {
        Input::Adapter(event) => {
            let _ = route(shell, event);
        }
        Input::Place {
            symbol,
            side,
            quantity,
            price,
            kind,
            time_in_force,
        } => {
            let ticket = Ticket {
                symbol,
                side,
                quantity,
                limit_price: price.as_deref(),
                kind: kind.as_deref(),
                time_in_force: time_in_force.as_deref(),
            };
            if let Ok(order) = new_order(&shell.venue, &ticket) {
                let _ = submit(shell, order);
            }
        }
        Input::Capacity {
            symbol,
            side,
            price,
            kind,
        } => {
            let ticket = Ticket {
                symbol,
                side,
                quantity: "0",
                limit_price: price.as_deref(),
                kind: kind.as_deref(),
                time_in_force: None,
            };
            let _ = capacity_of(shell, &ticket);
        }
        Input::Cancel(id) => {
            let _ = cancel(shell, *id);
        }
        Input::CancelAll(symbol) => {
            let _ = cancel_all(shell, symbol.as_deref());
        }
        Input::Amend {
            id,
            quantity,
            price,
        } => {
            if let (Ok(quantity), Ok(price)) = (
                parse_decimal("quantity", quantity),
                parse_decimal("limit price", price),
            ) {
                let _ = amend(shell, *id, quantity, price);
            }
        }
        Input::Conditional {
            symbol,
            side,
            quantity,
            trigger,
            level,
            percent,
            limit_price,
        } => {
            let ticket = ConditionalTicket {
                symbol,
                side,
                quantity,
                trigger,
                price: Some(level),
                offset: Some(level),
                percent: *percent,
                limit_price: limit_price.as_deref(),
            };
            let _ = place_conditional_order(shell, &ticket);
        }
        Input::CancelConditional(id) => {
            let _ = cancel_conditional_order(shell, *id);
        }
        Input::Bracket {
            symbol,
            side,
            quantity,
            price,
            stop,
            trail,
            percent,
            take_profit,
        } => {
            let ticket = BracketTicket {
                entry: Ticket {
                    symbol,
                    side,
                    quantity,
                    limit_price: price.as_deref(),
                    kind: None,
                    time_in_force: None,
                },
                stop_price: stop.as_deref(),
                trail: trail.as_deref(),
                trail_percent: *percent,
                take_profit: take_profit.as_deref(),
            };
            let _ = place_bracket_order(shell, &ticket);
        }
        Input::Sizing {
            symbol,
            side,
            entry,
            stop,
            risk,
            percent,
        } => {
            let ticket = SizingTicket {
                symbol,
                side,
                kind: None,
                time_in_force: None,
                entry,
                stop,
                risk,
                percent: *percent,
            };
            let _ = sizing_of(shell, &ticket);
        }
    }
}

proptest! {
    #![proptest_config(config())]

    #[test]
    fn whatever_arrives_the_shell_answers_without_panicking(inputs in vec(input(), 1..40)) {
        let (shell, events) = shell();
        for input in &inputs {
            feed(&shell, input);
            // What the venue answered goes through the pump's routing too,
            // so amendments and cancels reach orders the venue holds.
            for event in events.try_iter() {
                let _ = route(&shell, &event);
            }
        }
        // The front end's other questions, about a listed symbol and not.
        for symbol in ["VNM", "ZZZ"] {
            let data = lock(&shell.data);
            prop_assert!(data.is_ok());
            if let Ok(data) = data {
                let instrument = shell.instrument(symbol);
                let bars: Vec<BarDto> = data.bars(&instrument, DEFAULT_BAR_LIMIT).iter().map(|bar| BarDto::new(symbol, bar)).collect();
                let prints: Vec<PrintDto> = data.prints(&instrument, DEFAULT_PRINT_LIMIT).iter().map(PrintDto::from).collect();
                prop_assert!(serde_json::to_string(&bars).is_ok());
                prop_assert!(serde_json::to_string(&prints).is_ok());
            }
        }

        // What the front end receives next is built and serialized too.
        let snapshot = snapshot_of(&shell);
        prop_assert!(snapshot.is_ok(), "{:?}", snapshot.err());
        prop_assert!(serde_json::to_string(&snapshot.ok()).is_ok());
        let batch = take_batch(&shell);
        prop_assert!(batch.is_ok(), "{:?}", batch.err());
        prop_assert!(serde_json::to_string(&batch.ok()).is_ok());
    }
}
