//! Properties of sizing from risk (ADR 0021): whatever the prices, costs,
//! and lot, a stop hit loses no more than the budget, and one more lot
//! would lose more. The generators aim at the edges: a stop one cent from
//! the entry, and a budget that is exactly a whole number of lots' loss.

use fansipan_execution_engine::capacity::{BuyingPower, OrderCapacity};
use fansipan_execution_engine::sizing::{self, Inputs, RiskBudget};
use model::{Decimal, InstrumentId, InstrumentInfo, OrderType, Side, TickTable, TradingCosts};
use proptest::prelude::*;
use proptest::test_runner::RngSeed;

const SEED: u64 = 20_261_019;

/// A fixed seed, so every run explores the same cases (CLAUDE.md: tests are
/// deterministic). Set PROPTEST_RNG_SEED to explore others.
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

fn cents(n: i64) -> Decimal {
    Decimal::new(n, 2)
}

/// Rates in hundredths of a basis point, up to 0.5%.
fn rate(n: i64) -> Decimal {
    Decimal::new(n, 6)
}

fn unlimited() -> OrderCapacity {
    OrderCapacity {
        max_quantity: None,
        bound: None,
        buying_power: BuyingPower {
            amount: Decimal::MAX,
            estimated: false,
        },
        sellable: None,
        valued_at: None,
    }
}

#[derive(Debug)]
struct Case {
    side: Side,
    entry: Decimal,
    stop: Decimal,
    lot: Decimal,
    costs: TradingCosts,
    /// Whole lots the budget is built to cover, if it is built on the edge.
    lots: Option<i64>,
    budget_cents: i64,
}

fn case() -> impl Strategy<Value = Case> {
    (
        prop_oneof![Just(Side::Buy), Just(Side::Sell)],
        1i64..=10_000_000,
        prop_oneof![Just(1i64), 1i64..=1_000_000],
        prop_oneof![Just(1i64), Just(10), Just(100), 1i64..=1_000],
        0i64..=5_000,
        0i64..=5_000,
        prop_oneof![Just(None), (1i64..=50).prop_map(Some)],
        1i64..=1_000_000_000,
    )
        .prop_filter_map(
            "the stop must stay above zero",
            |(side, entry, gap, lot, c, t, lots, budget)| {
                let stop = match side {
                    Side::Buy => entry.checked_sub(gap).filter(|stop| *stop > 0)?,
                    Side::Sell => entry + gap,
                };
                Some(Case {
                    side,
                    entry: cents(entry),
                    stop: cents(stop),
                    lot: Decimal::from(lot),
                    costs: TradingCosts::new(rate(c), rate(t)),
                    lots,
                    budget_cents: budget,
                })
            },
        )
}

proptest! {
    #![proptest_config(config())]

    #[test]
    fn a_stop_hit_never_loses_more_than_the_budget_and_one_more_lot_would(case in case()) {
        let info = InstrumentInfo::new(InstrumentId::new("sim", "AAA"), case.lot, TickTable::flat(cents(1)).expect("tick"))
            .with_costs(case.costs);
        let room = unlimited();
        let inputs = |budget| Inputs {
            side: case.side,
            order_type: OrderType::MARKET,
            entry: case.entry,
            stop: case.stop,
            budget: RiskBudget::Amount(budget),
            equity: None,
            info: Some(&info),
            capacity: &room,
        };
        // The loss on one unit, read from a budget too small to buy any.
        let loss = sizing::size(&inputs(cents(1))).expect("a valid case").loss_per_unit;
        let budget = match case.lots {
            Some(lots) => loss * case.lot * Decimal::from(lots),
            None => cents(case.budget_cents),
        };

        let sizing = sizing::size(&inputs(budget)).expect("a valid case");

        prop_assert!(sizing.at_risk <= sizing.budget, "{} at risk over {}", sizing.at_risk, sizing.budget);
        prop_assert_eq!(sizing.quantity % case.lot, Decimal::ZERO);
        prop_assert!((sizing.quantity + case.lot) * loss > budget, "{} more fits", case.lot);
        if let Some(lots) = case.lots {
            prop_assert_eq!(sizing.quantity, case.lot * Decimal::from(lots));
        }
    }
}
