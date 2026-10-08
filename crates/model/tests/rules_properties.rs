//! Properties of market rules as data (roadmap Phase 7): for any tick ladder
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

proptest! {
    #[test]
    fn rounding_lands_on_the_ladder_on_the_right_side(ticks in ladder(), p in price()) {
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
    fn stepping_moves_one_tradable_price_at_a_time(ticks in ladder(), p in price()) {
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
        ticks in ladder(),
        p in price(),
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
