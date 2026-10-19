//! The day's profit and loss (ADR 0019), per instrument: what the holding
//! is worth now against what it was worth at the day's reference price,
//! plus what today's sales brought in, less what today's buys paid, less
//! the broker's commission and the sell tax.
//!
//! "Today" is this session's orders, adopted ones included (ADR 0024), at
//! each order's average price. Fills on orders placed elsewhere that the
//! adapter never reports reach the venue's position but not this ledger,
//! so they count as if held since the reference.

use model::{Decimal, InstrumentId, TradingCosts};

/// One instrument's day, net of costs.
#[derive(Debug, Clone, PartialEq)]
pub struct DayPnl {
    pub instrument: InstrumentId,
    /// `None` when a price it needs is unknown (the reference, for shares
    /// held before today's trades; the mark, for shares held now; a fill
    /// recorded without a price), or when the sum does not fit a `Decimal`.
    pub pnl: Option<Decimal>,
    /// Commission and sell tax on today's fills.
    pub costs: Decimal,
}

/// What one instrument's fills today add up to, by side.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Flows {
    pub bought: Decimal,
    pub bought_value: Decimal,
    pub sold: Decimal,
    pub sold_value: Decimal,
    /// A fill with no price: its value is unknown, so the day is too.
    pub unpriced: bool,
}

impl Flows {
    /// Adds a fill of `quantity` at `price`; `None` for a price unknown.
    pub fn add(self, buy: bool, quantity: Decimal, price: Option<Decimal>) -> Self {
        let Some(value) = price.and_then(|p| quantity.checked_mul(p)) else {
            return Self {
                unpriced: true,
                ..self
            };
        };
        let sum = |a: Decimal, b: Decimal| a.checked_add(b);
        let next = if buy {
            sum(self.bought, quantity)
                .zip(sum(self.bought_value, value))
                .map(|(bought, bought_value)| Self {
                    bought,
                    bought_value,
                    ..self
                })
        } else {
            sum(self.sold, quantity)
                .zip(sum(self.sold_value, value))
                .map(|(sold, sold_value)| Self {
                    sold,
                    sold_value,
                    ..self
                })
        };
        next.unwrap_or(Self {
            unpriced: true,
            ..self
        })
    }
}

/// Commission on everything traded today and tax on what was sold.
pub fn costs_of(flows: &Flows, costs: TradingCosts) -> Option<Decimal> {
    let traded = flows.bought_value.checked_add(flows.sold_value)?;
    let commission = traded.checked_mul(costs.commission_rate)?;
    let tax = flows.sold_value.checked_mul(costs.sell_tax_rate)?;
    commission.checked_add(tax)
}

/// The day for one instrument. `held` is the position now (negative when
/// short); what was held before today's fills follows from it. `reference`
/// is the day's reference price, `mark` the price now.
pub fn day_pnl(
    flows: &Flows,
    held: Decimal,
    reference: Option<Decimal>,
    mark: Option<Decimal>,
    costs: TradingCosts,
) -> (Option<Decimal>, Decimal) {
    let paid = costs_of(flows, costs);
    let pnl = (|| {
        if flows.unpriced {
            return None;
        }
        let before = held.checked_sub(flows.bought)?.checked_add(flows.sold)?;
        let worth_now = if held.is_zero() {
            Decimal::ZERO
        } else {
            held.checked_mul(mark?)?
        };
        let worth_before = if before.is_zero() {
            Decimal::ZERO
        } else {
            before.checked_mul(reference?)?
        };
        worth_now
            .checked_sub(worth_before)?
            .checked_add(flows.sold_value)?
            .checked_sub(flows.bought_value)?
            .checked_sub(paid?)
    })();
    (pnl, paid.unwrap_or(Decimal::ZERO))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(raw: &str) -> Decimal {
        raw.parse().expect("test decimal")
    }

    fn vietnam() -> TradingCosts {
        TradingCosts::new(d("0.0015"), d("0.001"))
    }

    #[test]
    fn a_holding_from_yesterday_moves_with_the_price() {
        let (pnl, costs) = day_pnl(
            &Flows::default(),
            d("100"),
            Some(d("10")),
            Some(d("11")),
            vietnam(),
        );

        assert_eq!((pnl, costs), (Some(d("100")), Decimal::ZERO));
    }

    #[test]
    fn a_round_trip_today_counts_its_costs() {
        let flows =
            Flows::default()
                .add(true, d("100"), Some(d("10")))
                .add(false, d("100"), Some(d("12")));

        let (pnl, costs) = day_pnl(&flows, Decimal::ZERO, None, None, vietnam());

        // 2,200 traded at 0.15% is 3.3; 1,200 sold at 0.1% is 1.2.
        assert_eq!(costs, d("4.5"));
        assert_eq!(pnl, Some(d("195.5")));
    }

    #[test]
    fn selling_yesterdays_shares_counts_from_the_reference() {
        let flows = Flows::default().add(false, d("100"), Some(d("11")));

        let (pnl, _) = day_pnl(
            &flows,
            Decimal::ZERO,
            Some(d("10")),
            None,
            TradingCosts::default(),
        );

        assert_eq!(pnl, Some(d("100")));
    }

    #[test]
    fn a_buy_today_counts_from_its_price() {
        let flows = Flows::default().add(true, d("100"), Some(d("10.5")));

        let (pnl, _) = day_pnl(
            &flows,
            d("100"),
            None,
            Some(d("11")),
            TradingCosts::default(),
        );

        assert_eq!(pnl, Some(d("50")));
    }

    #[test]
    fn a_price_it_needs_and_lacks_leaves_the_day_unknown() {
        let held =
            |reference, mark| day_pnl(&Flows::default(), d("100"), reference, mark, vietnam()).0;

        assert_eq!(held(Some(d("10")), None), None);
        assert_eq!(held(None, Some(d("11"))), None);
        let unpriced = Flows::default().add(true, d("1"), None);
        assert_eq!(
            day_pnl(&unpriced, d("1"), None, Some(d("5")), vietnam()).0,
            None
        );
    }

    #[test]
    fn values_too_large_to_add_leave_the_day_unknown_without_a_panic() {
        let flows = Flows::default()
            .add(true, Decimal::MAX, Some(Decimal::MAX))
            .add(false, Decimal::MAX, Some(d("2")));

        let (pnl, _) = day_pnl(
            &flows,
            Decimal::MAX,
            Some(Decimal::MAX),
            Some(Decimal::MAX),
            vietnam(),
        );

        assert_eq!(pnl, None);
    }
}
