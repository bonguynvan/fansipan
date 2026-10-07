//! A listing's rules and where its price wanders: the tick ladder, the
//! band, the lean, and the random walk.

use model::{Decimal, InstrumentId, InstrumentInfo, PriceBand, TickTable, UnixNanos};

/// Tick of an instrument listed with [`super::SimVenue::list`] (no market rules).
pub const TICK: Decimal = Decimal::from_parts(1, 0, 0, false, 2);

/// Lowest mid such a walk can reach: two ticks, so the bid never reaches
/// zero and nothing fills at a price of zero.
pub const MIN_MID: Decimal = Decimal::from_parts(2, 0, 0, false, 2);

/// Displayed size of such an instrument at the best bid and ask, and the
/// most one order can fill per price move.
pub const TOP_OF_BOOK_SIZE: Decimal = Decimal::ONE_HUNDRED;

/// One beat in this many moves a price at all; the rest leave it alone.
const MOVE_ODDS: u64 = 3;

/// How far a price strays from where it leans, as a fraction of its
/// reference price, before every move takes it back.
pub(super) const PULL_RANGE: Decimal = Decimal::from_parts(5, 0, 0, false, 2);

/// How far a price may lean without daily limits, as a fraction of its
/// reference price.
const OPEN_LEAN: Decimal = Decimal::from_parts(3, 0, 0, false, 2);

/// The strongest lean, as a fraction of the way from reference to a limit.
const MAX_LEAN: Decimal = Decimal::from_parts(95, 0, 0, false, 2);

/// Resolution of the draws that pick a lean and a direction.
const DRAW_STEPS: u64 = 1_000;

/// Deeper book levels and trade prints show between 1 and this many lots.
const MAX_LOTS: u64 = 40;

/// How one instrument trades on the simulated venue.
#[derive(Debug, Clone, PartialEq)]
pub struct Listing {
    pub info: InstrumentInfo,
    /// Today's limits. Its reference is also the opening price.
    pub band: PriceBand,
    /// Size shown at the best bid and ask, and the most one order fills per
    /// price move: an order larger than this fills across several moves.
    pub display_size: Decimal,
}

impl Listing {
    /// No market rules: a flat 0.01 tick, lot of one, no price limits.
    pub fn unrestricted(instrument: InstrumentId, price: Decimal) -> Result<Self, String> {
        let ticks = TickTable::flat(TICK).map_err(|e| e.to_string())?;
        Ok(Self {
            info: InstrumentInfo::new(instrument.clone(), Decimal::ONE, ticks),
            band: PriceBand::new(instrument, price, None, None, UnixNanos::default()),
            display_size: TOP_OF_BOOK_SIZE,
        })
    }

    pub(super) fn ticks(&self) -> &TickTable {
        &self.info.ticks
    }

    /// Where the mid may go so that the bid and ask stay inside the band
    /// and above zero.
    pub(super) fn clamp(&self, mid: Decimal) -> Decimal {
        let ticks = self.ticks();
        let lowest = self.band.floor.map_or_else(
            || ticks.step_up(ticks.step_up(Decimal::ZERO)),
            |f| ticks.step_up(f),
        );
        let clamped = mid.max(lowest);
        match self.band.ceiling {
            Some(ceiling) => clamped.min(ticks.step_down(ceiling).unwrap_or(ceiling)),
            None => clamped,
        }
    }

    /// Where this instrument's price drifts today. Drawn from the symbol, so
    /// every run shows the same mix: most names near their reference, now
    /// and then one pressing its ceiling or floor.
    pub(super) fn lean(&self) -> Decimal {
        let reference = self.band.reference;
        let draw = Decimal::from(fnv1a(self.info.id.symbol.as_bytes()) % (2 * DRAW_STEPS + 1))
            / Decimal::from(DRAW_STEPS)
            - Decimal::ONE;
        // Cubing keeps most draws small and leaves a few near the ends.
        let shaped = draw * draw * draw * MAX_LEAN;
        let reach = match (self.band.ceiling, self.band.floor) {
            (Some(ceiling), _) if shaped > Decimal::ZERO => ceiling - reference,
            (_, Some(floor)) if shaped < Decimal::ZERO => reference - floor,
            _ => reference * OPEN_LEAN,
        };
        self.clamp(self.ticks().round_nearest(reference + shaped * reach))
    }

    /// `price` moved `steps` prices along the ladder, up for positive, so a
    /// step from a tier boundary lands on the neighbouring price whichever
    /// way it goes. Not clamped to the band.
    pub(super) fn moved(&self, price: Decimal, steps: i64) -> Decimal {
        let ticks = self.ticks();
        (0..steps.unsigned_abs()).fold(price, |at, _| {
            if steps > 0 {
                ticks.step_up(at)
            } else {
                ticks.step_down(at).unwrap_or(at)
            }
        })
    }

    /// A size for a deeper level or a print: a whole number of lots that is
    /// the same every time for the same price and slot.
    pub(super) fn lots(&self, price: Decimal, slot: u64) -> Decimal {
        let units =
            u64::try_from(price.mantissa().unsigned_abs() % u128::from(u64::MAX)).unwrap_or(0);
        let mixed = xorshift(units ^ slot.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        self.info.lot_size.max(Decimal::ONE) * Decimal::from(1 + mixed % MAX_LOTS)
    }
}

/// One beat of the walk, in ticks: usually none, otherwise one either way,
/// back toward `lean` more often the further `mid` has strayed from it (a
/// full `PULL_RANGE` of `reference` away, always back). Drawn from a
/// xorshift sequence, so a given seed always walks the same path.
pub(crate) fn walk_ticks(seed: &mut u64, mid: Decimal, lean: Decimal, reference: Decimal) -> i64 {
    *seed = xorshift(*seed);
    if !seed.is_multiple_of(MOVE_ODDS) {
        return 0;
    }
    *seed = xorshift(*seed);
    let stray = mid
        .checked_sub(lean)
        .zip(reference.checked_mul(PULL_RANGE))
        .and_then(|(distance, range)| distance.checked_div(range))
        .unwrap_or(Decimal::ZERO);
    let pull = stray.clamp(-Decimal::ONE, Decimal::ONE);
    // An up-move's chance in DRAW_STEPS: half at the lean, none a full range above it.
    let up = (Decimal::ONE - pull) * Decimal::from(DRAW_STEPS / 2);
    if Decimal::from(*seed % DRAW_STEPS) < up {
        1
    } else {
        -1
    }
}

/// FNV-1a: a stable hash, the same on every platform and every run.
fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

fn xorshift(mut x: u64) -> u64 {
    x ^= x << 13;
    x ^= x >> 7;
    x ^ (x << 17)
}
