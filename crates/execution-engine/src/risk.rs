//! Pre-trade checks. Runs before any order reaches an ExecutionClient.
//!
//! Two families: this terminal's own limits ([`check`]), and the market's
//! rules for the instrument ([`check_market_rules`]: lot size, tick size,
//! daily price band) as the adapter published them. An order a venue would
//! refuse for breaking its rules is refused here first, with a reason.

use std::fmt;

use model::{Decimal, InstrumentInfo, NewOrder, PriceBand};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RiskLimits {
    pub max_order_quantity: Decimal,
    /// Cap on quantity × limit price. Market orders carry no price here and
    /// are bounded by quantity alone until risk can read a reference quote.
    pub max_order_notional: Decimal,
}

#[derive(Debug, Clone, PartialEq)]
pub enum RiskReject {
    QuantityNotPositive,
    QuantityTooLarge { max: Decimal },
    LimitPriceNotPositive,
    NotionalTooLarge { max: Decimal },
    NotLotMultiple { lot: Decimal },
    PriceNotOnTick { tick: Decimal },
    PriceAboveCeiling { ceiling: Decimal },
    PriceBelowFloor { floor: Decimal },
}

impl fmt::Display for RiskReject {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::QuantityNotPositive => f.write_str("quantity must be greater than zero"),
            Self::QuantityTooLarge { max } => {
                write!(f, "quantity is above the per-order limit of {max}")
            }
            Self::LimitPriceNotPositive => f.write_str("limit price must be greater than zero"),
            Self::NotionalTooLarge { max } => {
                write!(f, "order value is above the per-order limit of {max}")
            }
            Self::NotLotMultiple { lot } => {
                write!(f, "quantity must be a multiple of the lot size, {lot}")
            }
            Self::PriceNotOnTick { tick } => {
                write!(f, "limit price must be a multiple of the tick size, {tick}")
            }
            Self::PriceAboveCeiling { ceiling } => {
                write!(f, "limit price is above today's ceiling of {ceiling}")
            }
            Self::PriceBelowFloor { floor } => {
                write!(f, "limit price is below today's floor of {floor}")
            }
        }
    }
}

pub fn check(order: &NewOrder, limits: &RiskLimits) -> Result<(), RiskReject> {
    if order.quantity <= Decimal::ZERO {
        return Err(RiskReject::QuantityNotPositive);
    }
    if order.quantity > limits.max_order_quantity {
        return Err(RiskReject::QuantityTooLarge {
            max: limits.max_order_quantity,
        });
    }
    if let Some(price) = order.limit_price {
        if price <= Decimal::ZERO {
            return Err(RiskReject::LimitPriceNotPositive);
        }
        let within = order
            .quantity
            .checked_mul(price)
            .is_some_and(|notional| notional <= limits.max_order_notional);
        if !within {
            return Err(RiskReject::NotionalTooLarge {
                max: limits.max_order_notional,
            });
        }
    }
    Ok(())
}

/// The market's own rules for the instrument, where the adapter published
/// them. An instrument with no published rules passes: the absence of a
/// rule is not a reason to refuse an order.
pub fn check_market_rules(
    order: &NewOrder,
    info: Option<&InstrumentInfo>,
    band: Option<&PriceBand>,
) -> Result<(), RiskReject> {
    if let Some(info) = info {
        if !info.is_lot_multiple(order.quantity) {
            return Err(RiskReject::NotLotMultiple { lot: info.lot_size });
        }
    }
    let Some(price) = order.limit_price else {
        return Ok(());
    };
    if let Some(info) = info {
        if !info.ticks.is_on_tick(price) {
            return Err(RiskReject::PriceNotOnTick {
                tick: info.ticks.tick_at(price),
            });
        }
    }
    if let Some(band) = band {
        if let Some(ceiling) = band.ceiling.filter(|c| price > *c) {
            return Err(RiskReject::PriceAboveCeiling { ceiling });
        }
        if let Some(floor) = band.floor.filter(|f| price < *f) {
            return Err(RiskReject::PriceBelowFloor { floor });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use model::{InstrumentId, Side, TickTable, UnixNanos};

    const LIMITS: RiskLimits = RiskLimits {
        max_order_quantity: Decimal::ONE_HUNDRED,
        max_order_notional: Decimal::ONE_THOUSAND,
    };

    fn d(raw: &str) -> Decimal {
        raw.parse().expect("test decimal")
    }

    fn order(quantity: &str, limit_price: Option<&str>) -> NewOrder {
        NewOrder {
            instrument: InstrumentId::new("sim", "AAA"),
            side: Side::Buy,
            quantity: d(quantity),
            limit_price: limit_price.map(d),
        }
    }

    #[test]
    fn accepts_an_order_within_limits() {
        assert_eq!(check(&order("10", Some("5")), &LIMITS), Ok(()));
    }

    #[test]
    fn accepts_an_order_exactly_at_the_cap() {
        assert_eq!(check(&order("100", None), &LIMITS), Ok(()));
    }

    #[test]
    fn rejects_zero_or_negative_quantity() {
        assert_eq!(
            check(&order("0", None), &LIMITS),
            Err(RiskReject::QuantityNotPositive)
        );
        assert_eq!(
            check(&order("-1", None), &LIMITS),
            Err(RiskReject::QuantityNotPositive)
        );
    }

    #[test]
    fn rejects_quantity_above_the_cap() {
        assert_eq!(
            check(&order("100.5", None), &LIMITS),
            Err(RiskReject::QuantityTooLarge { max: d("100") })
        );
    }

    #[test]
    fn rejects_non_positive_limit_price() {
        assert_eq!(
            check(&order("1", Some("0")), &LIMITS),
            Err(RiskReject::LimitPriceNotPositive)
        );
    }

    #[test]
    fn rejects_a_limit_order_worth_more_than_the_notional_cap() {
        assert_eq!(
            check(&order("100", Some("10.01")), &LIMITS),
            Err(RiskReject::NotionalTooLarge { max: d("1000") })
        );
        assert_eq!(check(&order("100", Some("10")), &LIMITS), Ok(()));
    }

    #[test]
    fn an_absurd_limit_price_is_denied_not_overflowed() {
        let absurd = order("2", Some("79228162514264337593543950335"));

        assert_eq!(
            check(&absurd, &LIMITS),
            Err(RiskReject::NotionalTooLarge { max: d("1000") })
        );
    }

    fn vnm() -> (InstrumentInfo, PriceBand) {
        let ticks = TickTable::new(vec![
            (d("0"), d("10")),
            (d("10000"), d("50")),
            (d("50000"), d("100")),
        ])
        .unwrap();
        let id = InstrumentId::new("sim", "AAA");
        let band =
            PriceBand::from_fraction(id.clone(), d("62500"), d("0.07"), &ticks, UnixNanos(0))
                .unwrap();
        (InstrumentInfo::new(id, d("100"), ticks), band)
    }

    #[test]
    fn market_rules_accept_a_valid_order() {
        let (info, band) = vnm();

        assert_eq!(
            check_market_rules(&order("300", Some("62600")), Some(&info), Some(&band)),
            Ok(())
        );
        assert_eq!(
            check_market_rules(&order("300", None), Some(&info), Some(&band)),
            Ok(())
        );
    }

    #[test]
    fn market_rules_refuse_an_odd_lot() {
        let (info, band) = vnm();

        assert_eq!(
            check_market_rules(&order("150", Some("62600")), Some(&info), Some(&band)),
            Err(RiskReject::NotLotMultiple { lot: d("100") })
        );
    }

    #[test]
    fn market_rules_refuse_a_price_between_ticks() {
        let (info, band) = vnm();

        assert_eq!(
            check_market_rules(&order("100", Some("62550")), Some(&info), Some(&band)),
            Err(RiskReject::PriceNotOnTick { tick: d("100") })
        );
    }

    #[test]
    fn market_rules_refuse_a_price_outside_the_band() {
        let (info, band) = vnm();

        assert_eq!(
            check_market_rules(&order("100", Some("66900")), Some(&info), Some(&band)),
            Err(RiskReject::PriceAboveCeiling {
                ceiling: d("66800")
            })
        );
        assert_eq!(
            check_market_rules(&order("100", Some("58100")), Some(&info), Some(&band)),
            Err(RiskReject::PriceBelowFloor { floor: d("58200") })
        );
    }

    #[test]
    fn a_limit_exactly_at_the_ceiling_or_floor_is_inside_the_band() {
        let (info, band) = vnm();

        assert_eq!(
            check_market_rules(&order("100", Some("66800")), Some(&info), Some(&band)),
            Ok(())
        );
        assert_eq!(
            check_market_rules(&order("100", Some("58200")), Some(&info), Some(&band)),
            Ok(())
        );
    }

    #[test]
    fn a_market_order_has_no_price_for_the_band_to_refuse() {
        let (info, band) = vnm();

        assert_eq!(
            check_market_rules(&order("100", None), Some(&info), Some(&band)),
            Ok(())
        );
    }

    #[test]
    fn a_sell_must_be_a_whole_number_of_lots_too() {
        let (info, band) = vnm();
        let sell = NewOrder {
            side: Side::Sell,
            ..order("150", Some("62500"))
        };

        assert_eq!(
            check_market_rules(&sell, Some(&info), Some(&band)),
            Err(RiskReject::NotLotMultiple { lot: d("100") })
        );
    }

    #[test]
    fn a_lot_size_of_zero_or_less_means_no_lot_rule() {
        let (_, band) = vnm();
        let ticks = TickTable::flat(d("1")).unwrap();
        let unlotted = InstrumentInfo::new(InstrumentId::new("sim", "AAA"), d("0"), ticks);

        assert_eq!(
            check_market_rules(&order("7", Some("62500")), Some(&unlotted), Some(&band)),
            Ok(())
        );
    }

    #[test]
    fn an_instrument_without_published_rules_passes() {
        assert_eq!(
            check_market_rules(&order("7", Some("1.234")), None, None),
            Ok(())
        );
    }

    #[test]
    fn reasons_read_as_sentences() {
        let reason = RiskReject::QuantityTooLarge { max: d("1000") }.to_string();

        assert_eq!(reason, "quantity is above the per-order limit of 1000");
    }
}
