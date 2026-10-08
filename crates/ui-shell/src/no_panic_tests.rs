//! No panic on input from outside (roadmap Phase 7). Core parses no wire
//! protocol, but it reads what it cannot trust: what an adapter publishes
//! (quotes, prints, depth, rules, bands), whatever their values, and what
//! a trader types. Whatever arrives, the shell answers with a value or a
//! typed error. A panic inside an engine's lock would poison it and take
//! the terminal down over one bad message.
//!
//! The seed is fixed, so every run explores the same cases (CLAUDE.md:
//! tests are deterministic). Set PROPTEST_RNG_SEED to explore others.

use model::{
    BookLevel, Decimal, Depth, InstrumentId, InstrumentInfo, PriceBand, Quote, Side, TickTable,
    Trade, UnixNanos,
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

/// The Vietnamese sandbox, wired as `src/main.rs` wires it.
fn shell() -> Shell {
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
    connect_engines(&Bus::new(), venue).expect("the shell connects")
}

/// Any decimal an adapter could send: ordinary prices and sizes, zero, the
/// extremes, the smallest step, and any bit pattern at any scale.
fn decimal() -> impl Strategy<Value = Decimal> {
    prop_oneof![
        4 => (-1_000_000i64..1_000_000, 0u32..6).prop_map(|(m, scale)| Decimal::new(m, scale)),
        1 => Just(Decimal::ZERO),
        1 => Just(Decimal::MAX),
        1 => Just(Decimal::MIN),
        1 => Just(Decimal::new(1, 28)),
        2 => (any::<u32>(), any::<u32>(), any::<u32>(), any::<bool>(), 0u32..=28)
            .prop_map(|(lo, mid, hi, negative, scale)| Decimal::from_parts(lo, mid, hi, negative, scale)),
    ]
}

/// A listed symbol most of the time, or one the venue never listed.
fn symbol() -> impl Strategy<Value = String> {
    prop_oneof![4 => Just("VNM".to_string()), 1 => Just("ZZZ".to_string())]
}

fn instrument() -> impl Strategy<Value = InstrumentId> {
    symbol().prop_map(|s| InstrumentId::new(VENUE, s))
}

/// A tick ladder an adapter could publish: valid ones only, since
/// `TickTable::new` refuses the rest, but with any ticks and boundaries.
fn ticks() -> impl Strategy<Value = TickTable> {
    vec((decimal(), decimal()), 1..4).prop_map(|steps| {
        let mut steps: Vec<(Decimal, Decimal)> = steps
            .into_iter()
            .map(|(from, tick)| (from.abs(), tick.abs()))
            .collect();
        steps[0].0 = Decimal::ZERO;
        TickTable::new(steps)
            .unwrap_or_else(|_| TickTable::flat(Decimal::ONE).expect("a flat ladder"))
    })
}

fn optional(value: impl Strategy<Value = Decimal>) -> impl Strategy<Value = Option<Decimal>> {
    prop_oneof![1 => Just(None), 3 => value.prop_map(Some)]
}

/// Anything the adapter can tell core about a market.
fn adapter_event() -> impl Strategy<Value = Event> {
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
    ]
}

/// Anything a trader could type into a numeric field, or send by hand.
fn typed() -> impl Strategy<Value = String> {
    let pieces = select(vec!['0', '1', '9', '.', '-', '+', 'e', 'E', ' ', ',', '_']);
    prop_oneof![
        3 => decimal().prop_map(|d| d.to_string()),
        2 => vec(pieces, 0..40).prop_map(|chars| chars.into_iter().collect()),
        1 => any::<String>(),
        1 => Just("79228162514264337593543950336".to_string()),
        1 => Just("1e30".to_string()),
    ]
}

fn word() -> impl Strategy<Value = Option<String>> {
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
    prop_oneof![1 => Just(None), 3 => words.prop_map(|w| Some(w.to_string())), 1 => any::<String>().prop_map(Some)]
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
}

fn input() -> impl Strategy<Value = Input> {
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
    ]
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
    }
}

proptest! {
    #![proptest_config(config())]

    #[test]
    fn whatever_arrives_the_shell_answers_without_panicking(inputs in vec(input(), 1..40)) {
        let shell = shell();
        for input in &inputs {
            feed(&shell, input);
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
