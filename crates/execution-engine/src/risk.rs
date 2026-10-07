//! Pre-trade checks. Runs before any order reaches an ExecutionClient.

use std::fmt;

use model::{Decimal, NewOrder};

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

#[cfg(test)]
mod tests {
    use super::*;
    use model::{InstrumentId, Side};

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

    #[test]
    fn reasons_read_as_sentences() {
        let reason = RiskReject::QuantityTooLarge { max: d("1000") }.to_string();

        assert_eq!(reason, "quantity is above the per-order limit of 1000");
    }
}
