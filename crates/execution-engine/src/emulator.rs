//! Conditional orders held in this terminal (ADR 0020), as NautilusTrader's
//! order emulator holds what a venue does not take: a stop, a take-profit,
//! or a trailing stop waits here, watching the instrument's trades, and
//! when one fires it becomes an ordinary new order, cleared by risk at that
//! moment. Vietnam's exchanges take no stop orders; its brokers offer these
//! as "lệnh điều kiện".
//!
//! They live as long as the terminal runs.

use std::fmt;

use model::{ClientOrderId, Decimal, InstrumentId, Side, TickTable};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ConditionalId(pub u64);

impl fmt::Display for ConditionalId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// What fires a conditional order: a trade at or past a price, or one that
/// falls back from the best since it was placed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Trigger {
    /// A trade at or below the price: a stop-loss sell, or a buy on a dip.
    AtOrBelow(Decimal),
    /// A trade at or above the price: a take-profit sell, or a buy on a
    /// breakout.
    AtOrAbove(Decimal),
    /// A trade that gives back `Offset` from the best price since it was
    /// placed: below the highest for a sell, above the lowest for a buy.
    Trailing(Offset),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Offset {
    /// In price, e.g. 0.5.
    Amount(Decimal),
    /// In percent of the best price, e.g. 3 for 3%.
    Percent(Decimal),
}

/// The order a conditional sends when it fires.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Then {
    /// A limit order at this price.
    Limit(Decimal),
    /// One that trades at once: a limit at today's floor for a sell, or the
    /// ceiling for a buy, where the market has a daily band (taken in every
    /// session that takes limit orders, and never outside the band); a
    /// market order where it has none.
    Marketable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConditionalStatus {
    Waiting,
    /// Fired, and sent as this order, whatever became of it.
    Fired(ClientOrderId),
    Cancelled,
}

/// What a trader asks for.
#[derive(Debug, Clone, PartialEq)]
pub struct ConditionalRequest {
    pub instrument: InstrumentId,
    pub side: Side,
    pub quantity: Decimal,
    pub trigger: Trigger,
    pub then: Then,
}

/// A conditional order as the terminal holds it.
#[derive(Debug, Clone, PartialEq)]
pub struct Conditional {
    pub id: ConditionalId,
    pub instrument: InstrumentId,
    pub side: Side,
    pub quantity: Decimal,
    pub trigger: Trigger,
    pub then: Then,
    pub status: ConditionalStatus,
    /// For a trailing stop, the best trade since it was placed: the highest
    /// for a sell, the lowest for a buy.
    pub best: Option<Decimal>,
    /// The price a trade must reach to fire it now; `None` for a trailing
    /// stop that has seen no price yet.
    pub level: Option<Decimal>,
    /// Why it reached its trigger and still waits: its order could not go
    /// out then (trading halted, a session that takes no such order, a
    /// holding not yet settled), and it tries again on the next trade.
    pub blocked: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConditionalError {
    QuantityNotPositive,
    PriceNotPositive,
    /// An amount that is not above zero, or a percentage outside 0 to 100.
    OffsetOutOfRange,
    Unknown(ConditionalId),
    /// Already fired or cancelled.
    NotWaiting(ConditionalId),
    /// The last trade is already past the trigger, so it would fire at
    /// once, at whatever the market gives.
    AlreadyMet {
        last: Decimal,
    },
}

impl fmt::Display for ConditionalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::QuantityNotPositive => f.write_str("quantity must be greater than zero"),
            Self::PriceNotPositive => f.write_str("a price must be greater than zero"),
            Self::OffsetOutOfRange => {
                f.write_str("a trailing amount must be above zero, a percentage between 0 and 100")
            }
            Self::Unknown(id) => write!(f, "no conditional order {id}"),
            Self::NotWaiting(id) => {
                write!(f, "conditional order {id} already fired or was cancelled")
            }
            Self::AlreadyMet { last } => {
                write!(f, "the last trade, {last}, is already past the trigger")
            }
        }
    }
}

impl std::error::Error for ConditionalError {}

fn positive(price: Decimal) -> Result<Decimal, ConditionalError> {
    if price > Decimal::ZERO {
        Ok(price)
    } else {
        Err(ConditionalError::PriceNotPositive)
    }
}

impl Conditional {
    /// Places `request` as conditional `id`. `mark` is the instrument's last
    /// trade, which a trailing stop starts from; `ticks` its ladder, which a
    /// trailing level is rounded onto.
    pub(crate) fn new(
        id: ConditionalId,
        request: ConditionalRequest,
        mark: Option<Decimal>,
        ticks: Option<&TickTable>,
    ) -> Result<Self, ConditionalError> {
        if request.quantity <= Decimal::ZERO {
            return Err(ConditionalError::QuantityNotPositive);
        }
        let level = match request.trigger {
            Trigger::AtOrBelow(price) | Trigger::AtOrAbove(price) => Some(positive(price)?),
            Trigger::Trailing(Offset::Amount(amount)) if amount <= Decimal::ZERO => {
                return Err(ConditionalError::OffsetOutOfRange)
            }
            Trigger::Trailing(Offset::Percent(pct))
                if pct <= Decimal::ZERO || pct >= Decimal::ONE_HUNDRED =>
            {
                return Err(ConditionalError::OffsetOutOfRange)
            }
            Trigger::Trailing(_) => None,
        };
        if let Then::Limit(price) = request.then {
            positive(price)?;
        }
        let met = match (request.trigger, mark) {
            (Trigger::AtOrBelow(at), Some(last)) => last <= at,
            (Trigger::AtOrAbove(at), Some(last)) => last >= at,
            _ => false,
        };
        if let (true, Some(last)) = (met, mark) {
            return Err(ConditionalError::AlreadyMet { last });
        }
        let mut conditional = Self {
            id,
            instrument: request.instrument,
            side: request.side,
            quantity: request.quantity,
            trigger: request.trigger,
            then: request.then,
            status: ConditionalStatus::Waiting,
            best: None,
            level,
            blocked: None,
        };
        if let Some(mark) = mark {
            conditional.trail(mark, ticks);
        }
        Ok(conditional)
    }

    /// Moves a trailing stop's best price and level with a trade.
    fn trail(&mut self, price: Decimal, ticks: Option<&TickTable>) {
        let Trigger::Trailing(offset) = self.trigger else {
            return;
        };
        let best = match (self.best, self.side) {
            (Some(best), Side::Sell) => best.max(price),
            (Some(best), Side::Buy) => best.min(price),
            (None, _) => price,
        };
        self.best = Some(best);
        // A level that cannot be worked out keeps the last one: a stop that
        // still fires protects more than one that stops watching.
        self.level = trailing_level(best, offset, self.side, ticks).or(self.level);
    }

    /// A trade at `price` in this instrument: moves a trailing stop, and
    /// says whether the conditional fires. Only a waiting one can.
    pub(crate) fn observe(&mut self, price: Decimal, ticks: Option<&TickTable>) -> bool {
        if self.status != ConditionalStatus::Waiting {
            return false;
        }
        // The level before this trade: a trade that sets a new best is the
        // market moving the stop's way, not giving back.
        let level = self.level;
        self.trail(price, ticks);
        match (self.trigger, level) {
            (Trigger::AtOrAbove(at), _) => price >= at,
            (Trigger::AtOrBelow(at), _) => price <= at,
            (Trigger::Trailing(_), Some(level)) => match self.side {
                Side::Sell => price <= level,
                Side::Buy => price >= level,
            },
            (Trigger::Trailing(_), None) => false,
        }
    }
}

/// `offset` back from `best`, rounded onto the ladder the far side: down
/// for a sell, up for a buy, so the stop never sits closer than asked.
/// `None` when it does not fit a `Decimal` or is not above zero.
fn trailing_level(
    best: Decimal,
    offset: Offset,
    side: Side,
    ticks: Option<&TickTable>,
) -> Option<Decimal> {
    let distance = match offset {
        Offset::Amount(amount) => amount,
        Offset::Percent(pct) => best.checked_mul(pct)?.checked_div(Decimal::ONE_HUNDRED)?,
    };
    let raw = match side {
        Side::Sell => best.checked_sub(distance)?,
        Side::Buy => best.checked_add(distance)?,
    };
    let level = match (ticks, side) {
        (Some(ticks), Side::Sell) => ticks.round_down(raw),
        (Some(ticks), Side::Buy) => ticks.round_up(raw),
        (None, _) => raw,
    };
    (level > Decimal::ZERO).then_some(level)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(raw: &str) -> Decimal {
        raw.parse().expect("test decimal")
    }

    fn request(side: Side, trigger: Trigger) -> ConditionalRequest {
        ConditionalRequest {
            instrument: InstrumentId::new("sim", "AAA"),
            side,
            quantity: d("100"),
            trigger,
            then: Then::Marketable,
        }
    }

    fn placed(
        side: Side,
        trigger: Trigger,
        mark: Option<&str>,
        ticks: Option<&TickTable>,
    ) -> Conditional {
        Conditional::new(ConditionalId(1), request(side, trigger), mark.map(d), ticks)
            .expect("valid")
    }

    #[test]
    fn a_stop_fires_at_or_below_and_a_take_profit_at_or_above() {
        let mut stop = placed(Side::Sell, Trigger::AtOrBelow(d("10")), None, None);
        let mut target = placed(Side::Sell, Trigger::AtOrAbove(d("12")), None, None);

        assert!(!stop.observe(d("10.5"), None));
        assert!(stop.observe(d("10"), None));
        assert!(!target.observe(d("11.95"), None));
        assert!(target.observe(d("12.1"), None));
    }

    #[test]
    fn a_trailing_sell_follows_the_high_and_fires_on_the_give_back() {
        let ticks = TickTable::flat(d("0.05")).unwrap();
        let mut stop = placed(
            Side::Sell,
            Trigger::Trailing(Offset::Amount(d("0.52"))),
            Some("10"),
            Some(&ticks),
        );

        // 10 - 0.52 = 9.48, rounded down onto the 0.05 ladder.
        assert_eq!(stop.level, Some(d("9.45")));
        assert!(!stop.observe(d("11"), Some(&ticks)));
        assert_eq!((stop.best, stop.level), (Some(d("11")), Some(d("10.45"))));
        assert!(!stop.observe(d("10.5"), Some(&ticks)));
        assert_eq!(
            stop.best,
            Some(d("11")),
            "a lower trade does not move a sell's best"
        );
        assert!(stop.observe(d("10.45"), Some(&ticks)));
    }

    #[test]
    fn a_trailing_buy_follows_the_low_by_a_percentage() {
        let mut stop = placed(
            Side::Buy,
            Trigger::Trailing(Offset::Percent(d("10"))),
            None,
            None,
        );

        assert_eq!(stop.level, None, "no price seen yet");
        assert!(!stop.observe(d("50"), None));
        assert!(!stop.observe(d("40"), None));
        assert_eq!(stop.level, Some(d("44")));
        assert!(!stop.observe(d("43.9"), None));
        assert!(stop.observe(d("44"), None));
    }

    #[test]
    fn a_new_best_never_fires_the_stop_it_moves() {
        let mut stop = placed(
            Side::Sell,
            Trigger::Trailing(Offset::Amount(d("1"))),
            Some("10"),
            None,
        );

        assert!(!stop.observe(d("20"), None));
        assert_eq!(stop.level, Some(d("19")));
    }

    #[test]
    fn only_a_waiting_conditional_fires() {
        let mut stop = placed(Side::Sell, Trigger::AtOrBelow(d("10")), None, None);
        stop.status = ConditionalStatus::Cancelled;

        assert!(!stop.observe(d("1"), None));
    }

    #[test]
    fn nonsense_is_refused_when_placed() {
        let refused = |side, trigger, then| {
            Conditional::new(
                ConditionalId(1),
                ConditionalRequest {
                    then,
                    ..request(side, trigger)
                },
                None,
                None,
            )
            .unwrap_err()
        };
        let zero_quantity = ConditionalRequest {
            quantity: Decimal::ZERO,
            ..request(Side::Sell, Trigger::AtOrBelow(d("1")))
        };

        assert_eq!(
            Conditional::new(ConditionalId(1), zero_quantity, None, None).unwrap_err(),
            ConditionalError::QuantityNotPositive
        );
        assert_eq!(
            refused(Side::Sell, Trigger::AtOrBelow(d("0")), Then::Marketable),
            ConditionalError::PriceNotPositive
        );
        assert_eq!(
            refused(Side::Sell, Trigger::AtOrBelow(d("1")), Then::Limit(d("-1"))),
            ConditionalError::PriceNotPositive
        );
        assert_eq!(
            refused(
                Side::Sell,
                Trigger::Trailing(Offset::Percent(d("100"))),
                Then::Marketable
            ),
            ConditionalError::OffsetOutOfRange
        );
        assert_eq!(
            refused(
                Side::Buy,
                Trigger::Trailing(Offset::Amount(d("0"))),
                Then::Marketable
            ),
            ConditionalError::OffsetOutOfRange
        );
    }

    #[test]
    fn a_level_that_does_not_fit_stays_unset_without_a_panic() {
        let mut sell = placed(
            Side::Sell,
            Trigger::Trailing(Offset::Percent(d("50"))),
            None,
            None,
        );
        let buy = placed(
            Side::Buy,
            Trigger::Trailing(Offset::Amount(d("1"))),
            Some("79228162514264337593543950335"),
            None,
        );

        assert!(!sell.observe(Decimal::MAX, None));
        assert_eq!(sell.level, None);
        assert_eq!(buy.level, None);
    }
}
