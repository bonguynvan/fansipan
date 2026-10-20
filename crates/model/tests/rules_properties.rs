//! Properties of market rules as data: for any tick ladder
//! of the shape real markets use, every rounding lands on a tradable price
//! on the right side of the price it started from, and a band built from a
//! fraction stays inside its limits, on the ladder.
//!
//! Real ladders widen their tick with price, each tick dividing the next,
//! and each step starts on a multiple of its own tick (HOSE: 10 from 0, 50
//! from 10,000, 100 from 50,000). The generator builds exactly those.

use fansipan_model::{Decimal, InstrumentId, PriceBand, TickTable, UnixNanos};
use proptest::collection::vec;
use proptest::prelude::*;
use proptest::sample::subsequence;
use proptest::test_runner::RngSeed;

const SEED: u64 = 20_261_008;

/// A fixed seed, so every run explores the same cases (tests are
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

/// Ticks from fine to coarse, each dividing the next: 0.01, 0.05, 0.1, 0.5,
/// 1, 5, 10, 50, 100, as `(mantissa, scale)`.
const TICKS: [(i64, u32); 9] = [
    (1, 2),
    (5, 2),
    (1, 1),
    (5, 1),
    (1, 0),
    (5, 0),
    (10, 0),
    (50, 0),
    (100, 0),
];

fn ladder() -> impl Strategy<Value = TickTable> {
    (
        subsequence((0..TICKS.len()).collect::<Vec<_>>(), 1..=4),
        vec(1u32..400, 3),
    )
        .prop_map(|(tiers, widths)| {
            let mut steps: Vec<(Decimal, Decimal)> = Vec::new();
            for (i, tier) in tiers.iter().enumerate() {
                let (mantissa, scale) = TICKS[*tier];
                let tick = Decimal::new(mantissa, scale);
                let from = match steps.last() {
                    None => Decimal::ZERO,
                    // A whole number of this step's ticks, past the last step.
                    Some((previous, _)) => {
                        ((*previous / tick).floor() + Decimal::from(widths[i - 1])) * tick
                    }
                };
                steps.push((from, tick));
            }
            TickTable::new(steps).expect("a ladder of the real shape")
        })
}

/// A price from one hundredth to a hundred thousand, in hundredths.
fn price() -> impl Strategy<Value = Decimal> {
    (1i64..10_000_000).prop_map(|cents| Decimal::new(cents, 2))
}

/// A ladder, and a price on it to test: anywhere, a few of its finest ticks
/// (and a fraction) either side of where a step starts, where rounding can
/// cross into the next step, or exactly on a tick of some step.
fn ladder_and_price() -> impl Strategy<Value = (TickTable, Decimal)> {
    ladder().prop_flat_map(|ticks| {
        let steps = ticks.steps().to_vec();
        let finest = steps[0].1;
        let count = steps.len();
        let near_step = {
            let steps = steps.clone();
            (0..count, -3i64..=3, 0i64..100).prop_map(move |(i, ticks_away, hundredths)| {
                steps[i].0
                    + finest * Decimal::from(ticks_away)
                    + finest * Decimal::new(hundredths, 2)
            })
        };
        // Exactly where a step starts, half the time: the tier it is in decides its next tick.
        let on_a_tick = (0..count, prop_oneof![Just(0u32), 0u32..50])
            .prop_map(move |(i, n)| steps[i].0 + steps[i].1 * Decimal::from(n));
        let positive = prop_oneof![price(), near_step, on_a_tick].prop_map(|p| {
            if p > Decimal::ZERO {
                p
            } else {
                Decimal::new(1, 2)
            }
        });
        (Just(ticks), positive)
    })
}

proptest! {
    #![proptest_config(config())]

    #[test]
    fn rounding_lands_on_the_ladder_on_the_right_side((ticks, p) in ladder_and_price()) {
        let down = ticks.round_down(p);
        let up = ticks.round_up(p);
        let nearest = ticks.round_nearest(p);

        prop_assert!(down <= p && p <= up, "{down} <= {p} <= {up}");
        prop_assert!(ticks.is_on_tick(up), "up {up}");
        if down > Decimal::ZERO {
            prop_assert!(ticks.is_on_tick(down), "down {down}");
        }
        prop_assert!(down <= nearest && nearest <= up);
        // Nothing tradable lies strictly between the two roundings.
        if down > Decimal::ZERO && down < up {
            prop_assert_eq!(ticks.step_up(down), up);
        }
        // A tradable price rounds to itself.
        if ticks.is_on_tick(p) {
            prop_assert_eq!((down, up, nearest), (p, p, p));
        }
    }

    #[test]
    fn stepping_moves_one_tradable_price_at_a_time((ticks, p) in ladder_and_price()) {
        let start = ticks.round_up(p);
        let next = ticks.step_up(start);

        prop_assert!(next > start && ticks.is_on_tick(next), "{start} -> {next}");
        prop_assert_eq!(ticks.step_down(next), Some(start));
        if let Some(below) = ticks.step_down(start) {
            prop_assert!(below < start && ticks.is_on_tick(below), "{start} -> {below}");
        }
    }

    #[test]
    fn a_band_from_a_fraction_stays_inside_its_limits_on_the_ladder(
        (ticks, p) in ladder_and_price(),
        percent in 0u32..30,
    ) {
        let reference = ticks.round_up(p);
        let fraction = Decimal::new(i64::from(percent), 2);
        let band = PriceBand::from_fraction(
            InstrumentId::new("sim", "AAA"),
            reference,
            fraction,
            &ticks,
            UnixNanos(0),
        )
        .expect("a valid band");
        let ceiling = band.ceiling.expect("a ceiling");
        let floor = band.floor.expect("a floor");

        prop_assert!(floor <= reference && reference <= ceiling, "{floor} <= {reference} <= {ceiling}");
        prop_assert!(ceiling <= reference * (Decimal::ONE + fraction));
        prop_assert!(floor >= reference * (Decimal::ONE - fraction));
        prop_assert!(ticks.is_on_tick(ceiling), "ceiling {ceiling}");
        if floor > Decimal::ZERO {
            prop_assert!(ticks.is_on_tick(floor), "floor {floor}");
        }
    }
}

/// Any decimal at all: ordinary values, zero, the extremes, the smallest
/// step, and any bit pattern at any scale.
fn any_decimal() -> impl Strategy<Value = Decimal> {
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

/// Any ladder `TickTable::new` accepts from an adapter, however odd: ticks
/// and boundaries of any size, not only the shape real markets use.
fn any_ladder() -> impl Strategy<Value = TickTable> {
    vec((any_decimal(), any_decimal()), 1..4).prop_filter_map(
        "a ladder TickTable::new accepts",
        |steps| {
            let mut steps: Vec<(Decimal, Decimal)> = steps
                .into_iter()
                .map(|(from, tick)| (from.abs(), tick.abs()))
                .collect();
            steps[0].0 = Decimal::ZERO;
            steps.sort_by_key(|step| step.0);
            steps.dedup_by(|a, b| a.0 == b.0);
            TickTable::new(steps).ok()
        },
    )
}

proptest! {
    #![proptest_config(config())]

    /// No panic on rules data from outside: whatever
    /// ladder an adapter publishes and whatever price arrives, rounding,
    /// stepping, and building a band answer without panicking.
    #[test]
    fn any_ladder_and_any_price_answer_without_panicking(
        ticks in any_ladder(),
        p in any_decimal(),
        fraction in any_decimal(),
    ) {
        let _ = ticks.tick_at(p);
        let _ = ticks.is_on_tick(p);
        let _ = ticks.round_down(p);
        let _ = ticks.round_up(p);
        let _ = ticks.round_nearest(p);
        let _ = ticks.step_up(p);
        let _ = ticks.step_down(p);
        let _ = PriceBand::from_fraction(InstrumentId::new("sim", "AAA"), p, fraction, &ticks, UnixNanos(0));
    }
}
