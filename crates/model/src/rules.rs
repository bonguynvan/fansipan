//! Market rules as data: tick sizes, lot sizes, daily price limits, and book
//! depth. Nothing here names a market; an adapter fills these in for its
//! venue (a HOSE tick ladder, a 7% band) and core applies them the same way
//! everywhere.

use std::fmt;

use rust_decimal::{Decimal, RoundingStrategy};

use crate::{InstrumentId, OrderType, UnixNanos};

/// The smallest price increment as a function of price: a ladder of
/// `(from, tick)` steps. A flat-tick market has one step; a market whose tick
/// widens with price (HOSE: 10 below 10,000, 50 below 50,000, else 100) has
/// several.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TickTable {
    steps: Vec<(Decimal, Decimal)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum TickTableError {
    Empty,
    /// The first step must start at zero, so every price has a tick.
    FirstStepNotZero,
    NotAscending,
    NonPositiveTick,
}

impl fmt::Display for TickTableError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Empty => "a tick table needs at least one step",
            Self::FirstStepNotZero => "a tick table's first step must start at 0",
            Self::NotAscending => "tick table steps must be in ascending price order",
            Self::NonPositiveTick => "every tick must be greater than zero",
        })
    }
}

impl std::error::Error for TickTableError {}

/// Rounding a limit to a tradable price can cross a step boundary (a raw
/// ceiling of 10,165 is in the 50 tier, its 10-tier rounding is not); this
/// many re-roundings settles any realistic ladder.
const MAX_ROUNDING_PASSES: usize = 5;

impl TickTable {
    /// Steps as `(from_price, tick)`, ascending, the first starting at zero.
    pub fn new(steps: Vec<(Decimal, Decimal)>) -> Result<Self, TickTableError> {
        let Some(first) = steps.first() else {
            return Err(TickTableError::Empty);
        };
        if !first.0.is_zero() {
            return Err(TickTableError::FirstStepNotZero);
        }
        if steps.iter().any(|(_, tick)| *tick <= Decimal::ZERO) {
            return Err(TickTableError::NonPositiveTick);
        }
        if steps.windows(2).any(|w| w[1].0 <= w[0].0) {
            return Err(TickTableError::NotAscending);
        }
        Ok(Self { steps })
    }

    /// One tick at every price.
    pub fn flat(tick: Decimal) -> Result<Self, TickTableError> {
        Self::new(vec![(Decimal::ZERO, tick)])
    }

    pub fn steps(&self) -> &[(Decimal, Decimal)] {
        &self.steps
    }

    /// The tick that applies at `price`.
    pub fn tick_at(&self, price: Decimal) -> Decimal {
        self.steps
            .iter()
            .rev()
            .find(|(from, _)| price >= *from)
            .or(self.steps.first())
            .map_or(Decimal::ONE, |(_, tick)| *tick)
    }

    /// Whether `price` is tradable: above zero and a multiple of the tick
    /// that applies at it.
    pub fn is_on_tick(&self, price: Decimal) -> bool {
        price > Decimal::ZERO
            && price
                .checked_rem(self.tick_at(price))
                .is_some_and(|rest| rest.is_zero())
    }

    /// The highest price on the ladder not above `price`.
    pub fn round_down(&self, price: Decimal) -> Decimal {
        self.settle(price, |units| units.floor())
    }

    /// The lowest price on the ladder not below `price`.
    pub fn round_up(&self, price: Decimal) -> Decimal {
        self.settle(price, |units| units.ceil())
    }

    /// The nearest price on the ladder; halfway between two, the higher.
    pub fn round_nearest(&self, price: Decimal) -> Decimal {
        self.settle(price, |units| {
            units.round_dp_with_strategy(0, RoundingStrategy::MidpointAwayFromZero)
        })
    }

    /// The next price on the ladder above `price`, across step boundaries.
    /// From a price between ticks that is the first one above it.
    pub fn step_up(&self, price: Decimal) -> Decimal {
        let up = self.round_up(price);
        if up > price {
            return up;
        }
        price
            .checked_add(self.finest())
            .map_or(price, |next| self.round_up(next))
    }

    /// The next tradable price below `price`, across step boundaries, or
    /// `None` when no price above zero is below it.
    pub fn step_down(&self, price: Decimal) -> Option<Decimal> {
        let down = self.round_down(price);
        let below = if down < price {
            down
        } else {
            self.round_down(price.checked_sub(self.finest())?)
        };
        (below > Decimal::ZERO).then_some(below)
    }

    /// The smallest tick anywhere in the table.
    fn finest(&self) -> Decimal {
        self.steps
            .iter()
            .map(|(_, tick)| *tick)
            .min()
            .unwrap_or(Decimal::ONE)
    }

    /// A price too large for this arithmetic (near `Decimal::MAX`) comes
    /// back unchanged rather than panicking.
    fn settle(&self, price: Decimal, round: impl Fn(Decimal) -> Decimal) -> Decimal {
        let on = |tick: Decimal| {
            price
                .checked_div(tick)
                .map(&round)
                .and_then(|units| units.checked_mul(tick))
        };
        let mut tick = self.tick_at(price);
        let Some(mut candidate) = on(tick) else {
            return price;
        };
        for _ in 0..MAX_ROUNDING_PASSES {
            let tick_there = self.tick_at(candidate);
            if tick_there == tick {
                break;
            }
            tick = tick_there;
            match on(tick) {
                Some(next) => candidate = next,
                None => return price,
            }
        }
        candidate
    }
}

/// What a venue says about an instrument: how it trades, not what it costs.
/// Published once per instrument (and again if it changes).
///
/// New fields may be added in a minor release, so build one with
/// [`InstrumentInfo::new`] and the `with_*` methods, not a struct literal.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct InstrumentInfo {
    pub id: InstrumentId,
    /// Display name, e.g. a company name.
    pub name: String,
    /// The listing board or segment, e.g. "HOSE". Empty when it means nothing.
    pub exchange: String,
    /// ISO 4217 code of the price currency, e.g. "VND".
    pub currency: String,
    /// Order quantities must be a multiple of this. One for no lot rule.
    pub lot_size: Decimal,
    pub ticks: TickTable,
    /// Whether a sell may take more than the settled holding. Vietnam
    /// forbids it for retail accounts; a US margin account allows it.
    pub short_selling: bool,
    /// The order types an odd lot (above zero, below one lot) may be.
    /// Empty when the market takes no odd lots; Vietnam takes them as limit
    /// orders for the day.
    pub odd_lot_types: Vec<OrderType>,
}

impl InstrumentInfo {
    pub fn new(id: InstrumentId, lot_size: Decimal, ticks: TickTable) -> Self {
        Self {
            name: id.symbol.clone(),
            id,
            exchange: String::new(),
            currency: String::new(),
            lot_size,
            ticks,
            short_selling: true,
            odd_lot_types: Vec::new(),
        }
    }

    pub fn with_name(self, name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ..self
        }
    }

    pub fn with_exchange(self, exchange: impl Into<String>) -> Self {
        Self {
            exchange: exchange.into(),
            ..self
        }
    }

    pub fn with_currency(self, currency: impl Into<String>) -> Self {
        Self {
            currency: currency.into(),
            ..self
        }
    }

    pub fn with_short_selling(self, short_selling: bool) -> Self {
        Self {
            short_selling,
            ..self
        }
    }

    pub fn with_odd_lot_types(self, order_types: impl IntoIterator<Item = OrderType>) -> Self {
        Self {
            odd_lot_types: order_types.into_iter().collect(),
            ..self
        }
    }

    pub fn is_lot_multiple(&self, quantity: Decimal) -> bool {
        self.lot_size <= Decimal::ZERO || (quantity % self.lot_size).is_zero()
    }

    /// Above zero and below one lot.
    pub fn is_odd_lot(&self, quantity: Decimal) -> bool {
        quantity > Decimal::ZERO && quantity < self.lot_size
    }

    /// Whether the market takes an order for `quantity` of this type: whole
    /// lots, or an odd lot of a type it takes odd lots as.
    pub fn takes_quantity(&self, quantity: Decimal, order_type: OrderType) -> bool {
        self.is_lot_multiple(quantity)
            || (self.is_odd_lot(quantity) && self.odd_lot_types.contains(&order_type))
    }
}

/// Today's price limits for an instrument. `reference` is what change and
/// colour are measured against (usually the previous close); `ceiling` and
/// `floor` bound order prices on markets with daily limits, and are `None`
/// where there are none.
///
/// New fields may be added in a minor release, so build one with
/// [`PriceBand::new`] or [`PriceBand::from_fraction`].
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct PriceBand {
    pub instrument: InstrumentId,
    pub reference: Decimal,
    pub ceiling: Option<Decimal>,
    pub floor: Option<Decimal>,
    pub ts_event: UnixNanos,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum BandError {
    NonPositiveReference,
    /// A fraction must be at least zero and below one.
    FractionOutOfRange,
    /// The limits do not fit in a `Decimal`.
    Overflow,
}

impl fmt::Display for BandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NonPositiveReference => "a band's reference price must be above zero",
            Self::FractionOutOfRange => "a band's fraction must be at least 0 and below 1",
            Self::Overflow => "the band's limits are too large to represent",
        })
    }
}

impl std::error::Error for BandError {}

impl PriceBand {
    pub fn new(
        instrument: InstrumentId,
        reference: Decimal,
        ceiling: Option<Decimal>,
        floor: Option<Decimal>,
        ts_event: UnixNanos,
    ) -> Self {
        Self {
            instrument,
            reference,
            ceiling,
            floor,
            ts_event,
        }
    }

    /// A symmetric band of `fraction` (0.07 for 7%) around `reference`, with
    /// the ceiling rounded down and the floor rounded up to tradable
    /// prices, so neither ever sits outside the true limit.
    pub fn from_fraction(
        instrument: InstrumentId,
        reference: Decimal,
        fraction: Decimal,
        ticks: &TickTable,
        ts_event: UnixNanos,
    ) -> Result<Self, BandError> {
        if reference <= Decimal::ZERO {
            return Err(BandError::NonPositiveReference);
        }
        if fraction < Decimal::ZERO || fraction >= Decimal::ONE {
            return Err(BandError::FractionOutOfRange);
        }
        let ceiling = reference
            .checked_mul(Decimal::ONE + fraction)
            .ok_or(BandError::Overflow)?;
        let floor = reference
            .checked_mul(Decimal::ONE - fraction)
            .ok_or(BandError::Overflow)?;
        Ok(Self::new(
            instrument,
            reference,
            Some(ticks.round_down(ceiling)),
            Some(ticks.round_up(floor)),
            ts_event,
        ))
    }

    pub fn contains(&self, price: Decimal) -> bool {
        self.ceiling.is_none_or(|c| price <= c) && self.floor.is_none_or(|f| price >= f)
    }
}

#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct BookLevel {
    pub price: Decimal,
    pub size: Decimal,
}

impl BookLevel {
    pub fn new(price: Decimal, size: Decimal) -> Self {
        Self { price, size }
    }
}

/// The top of the book, best level first on each side. Venues that publish
/// depth send this alongside quotes for subscribed instruments.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Depth {
    pub instrument: InstrumentId,
    pub bids: Vec<BookLevel>,
    pub asks: Vec<BookLevel>,
    pub ts_event: UnixNanos,
}

impl Depth {
    pub fn new(
        instrument: InstrumentId,
        bids: Vec<BookLevel>,
        asks: Vec<BookLevel>,
        ts_event: UnixNanos,
    ) -> Self {
        Self {
            instrument,
            bids,
            asks,
            ts_event,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(raw: &str) -> Decimal {
        raw.parse().expect("test decimal")
    }

    fn hose() -> TickTable {
        TickTable::new(vec![
            (d("0"), d("10")),
            (d("10000"), d("50")),
            (d("50000"), d("100")),
        ])
        .unwrap()
    }

    #[test]
    fn the_tick_depends_on_the_price() {
        let ticks = hose();

        assert_eq!(ticks.tick_at(d("9990")), d("10"));
        assert_eq!(ticks.tick_at(d("10000")), d("50"));
        assert_eq!(ticks.tick_at(d("62500")), d("100"));
    }

    #[test]
    fn prices_are_on_tick_only_for_their_own_step() {
        let ticks = hose();

        assert!(ticks.is_on_tick(d("9990")));
        assert!(ticks.is_on_tick(d("26850")));
        assert!(!ticks.is_on_tick(d("26860")));
        assert!(!ticks.is_on_tick(d("62550")));
        assert!(!ticks.is_on_tick(d("0")));
        assert!(!ticks.is_on_tick(d("-10")));
    }

    #[test]
    fn rounding_settles_on_the_tick_of_the_rounded_price() {
        let ticks = hose();

        // 9,500 + 7% = 10,165: in the 50 tier, so 10,150, not 10,160.
        assert_eq!(ticks.round_down(d("10165")), d("10150"));
        assert_eq!(ticks.round_up(d("62520")), d("62600"));
        assert_eq!(ticks.round_nearest(d("26860")), d("26850"));
        assert_eq!(ticks.round_down(d("49999")), d("49950"));
        assert_eq!(ticks.round_up(d("9991")), d("10000"));
    }

    #[test]
    fn a_price_halfway_between_ticks_rounds_up() {
        let ticks = hose();

        assert_eq!(ticks.round_nearest(d("26825")), d("26850"));
        assert_eq!(ticks.round_nearest(d("26875")), d("26900"));
    }

    #[test]
    fn stepping_from_between_ticks_finds_the_nearest_price_across_a_tier() {
        let ticks = hose();

        assert_eq!(ticks.step_up(d("9995")), d("10000"));
        assert_eq!(ticks.step_down(d("10005")), Some(d("10000")));
        assert_eq!(ticks.step_up(d("0")), d("10"));
    }

    #[test]
    fn nothing_tradable_is_below_the_first_tick() {
        let ticks = hose();

        assert_eq!(ticks.step_down(d("10")), None);
        assert_eq!(ticks.step_down(d("0")), None);
        assert_eq!(ticks.step_down(d("20")), Some(d("10")));
    }

    #[test]
    fn rounding_near_the_largest_decimal_never_panics() {
        let ticks = hose();

        assert_eq!(ticks.step_up(Decimal::MAX), Decimal::MAX);
        assert_eq!(ticks.round_up(Decimal::MAX), Decimal::MAX);
        assert_eq!(
            ticks.round_nearest(Decimal::MAX),
            d("79228162514264337593543950300")
        );
    }

    #[test]
    fn stepping_crosses_tier_boundaries_onto_valid_prices() {
        let ticks = hose();

        assert_eq!(ticks.step_down(d("10000")), Some(d("9990")));
        assert_eq!(ticks.step_up(d("9990")), d("10000"));
        assert_eq!(ticks.step_up(d("10000")), d("10050"));
        assert_eq!(ticks.step_down(d("62500")), Some(d("62400")));
        assert_eq!(ticks.step_up(d("49950")), d("50000"));
        assert_eq!(ticks.step_up(d("50000")), d("50100"));
    }

    #[test]
    fn a_band_rounds_its_ceiling_down_and_its_floor_up() {
        let band = PriceBand::from_fraction(
            InstrumentId::new("sim", "VNM"),
            d("62500"),
            d("0.07"),
            &hose(),
            UnixNanos(0),
        )
        .unwrap();

        // 66,875 → 66,800; 58,125 → 58,200.
        assert_eq!(band.ceiling, Some(d("66800")));
        assert_eq!(band.floor, Some(d("58200")));
        assert!(band.contains(d("66800")));
        assert!(!band.contains(d("66900")));
    }

    #[test]
    fn a_band_refuses_a_reference_or_fraction_that_makes_no_sense() {
        let band = |reference: &str, fraction: &str| {
            PriceBand::from_fraction(
                InstrumentId::new("sim", "VNM"),
                d(reference),
                d(fraction),
                &hose(),
                UnixNanos(0),
            )
        };

        assert_eq!(band("0", "0.07"), Err(BandError::NonPositiveReference));
        assert_eq!(band("62500", "-0.1"), Err(BandError::FractionOutOfRange));
        assert_eq!(band("62500", "1.5"), Err(BandError::FractionOutOfRange));
        assert_eq!(
            band("79228162514264337593543950335", "0.5"),
            Err(BandError::Overflow)
        );
    }

    #[test]
    fn a_band_without_limits_contains_every_price() {
        let band = PriceBand::new(
            InstrumentId::new("sim", "AAPL"),
            d("190"),
            None,
            None,
            UnixNanos(0),
        );

        assert!(band.contains(d("1000000")));
    }

    #[test]
    fn malformed_tables_are_refused() {
        assert_eq!(TickTable::new(vec![]), Err(TickTableError::Empty));
        assert_eq!(
            TickTable::new(vec![(d("5"), d("1"))]),
            Err(TickTableError::FirstStepNotZero)
        );
        assert_eq!(
            TickTable::new(vec![(d("0"), d("1")), (d("0"), d("2"))]),
            Err(TickTableError::NotAscending)
        );
        assert_eq!(
            TickTable::flat(Decimal::ZERO),
            Err(TickTableError::NonPositiveTick)
        );
    }

    #[test]
    fn lot_multiples() {
        let info = InstrumentInfo::new(InstrumentId::new("sim", "VNM"), d("100"), hose());

        assert!(info.is_lot_multiple(d("300")));
        assert!(!info.is_lot_multiple(d("150")));
        assert_eq!(info.name, "VNM");
    }

    #[test]
    fn an_odd_lot_is_taken_only_as_the_types_the_market_allows() {
        let ticks = TickTable::flat(d("100")).unwrap();
        let info = InstrumentInfo::new(InstrumentId::new("sim", "VNM"), d("100"), ticks)
            .with_odd_lot_types([OrderType::LIMIT]);

        assert!(info.takes_quantity(d("300"), OrderType::MARKET));
        assert!(info.takes_quantity(d("50"), OrderType::LIMIT));
        assert!(!info.takes_quantity(d("50"), OrderType::MARKET));
        assert!(!info.takes_quantity(d("150"), OrderType::LIMIT));
        assert!(!info.is_odd_lot(d("0")));
    }
}
