//! How large an order may be for what the trader will lose if its stop is
//! hit: the money at risk over the loss on each unit, rounded down to what the market takes.
//! The loss on each unit counts the round trip's costs as the
//! adapter states them, so a stop hit costs no more than the budget, and
//! the size never exceeds what the account can take.

use std::fmt;

use model::{AccountState, Decimal, InstrumentId, InstrumentInfo, OrderType, Side, TradingCosts};

use crate::capacity::{self, Bound, OrderCapacity};

/// What the trader will lose at most if the stop is hit.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RiskBudget {
    /// An amount of the account's currency.
    Amount(Decimal),
    /// A percentage of the account's equity, e.g. 1 for 1%.
    PercentOfEquity(Decimal),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Sizing {
    /// The size: whole lots, or an odd lot where the market takes this
    /// order type as one and not a lot fits. Zero when the budget does not
    /// cover the smallest order.
    pub quantity: Decimal,
    /// What one unit loses if the stop is hit: the distance to the stop
    /// and the costs of buying and selling it.
    pub loss_per_unit: Decimal,
    /// The money the budget allows to lose.
    pub budget: Decimal,
    /// What `quantity` loses if the stop is hit; never above `budget`.
    pub at_risk: Decimal,
    /// Set when the account, the holding, or this terminal's limits allow
    /// less than the budget does, and `quantity` is that smaller size.
    pub capped_by: Option<Bound>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SizingError {
    /// The entry or the stop is not above zero.
    PriceNotPositive,
    /// A buy's stop must be below its entry, a sell's above.
    StopOnWrongSide(Side),
    /// An amount at or below zero, or a percentage outside (0, 100].
    BudgetOutOfRange,
    /// A percentage of equity, while a held instrument has no price yet.
    EquityUnknown(InstrumentId),
    /// Equity at or below zero leaves nothing to take a percentage of.
    NoEquity,
    /// The arithmetic would overflow a `Decimal`.
    TooLarge,
}

impl fmt::Display for SizingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PriceNotPositive => f.write_str("the entry and the stop must be above zero"),
            Self::StopOnWrongSide(Side::Buy) => f.write_str("a buy's stop must be below its entry"),
            Self::StopOnWrongSide(Side::Sell) => {
                f.write_str("a sell's stop must be above its entry")
            }
            Self::BudgetOutOfRange => {
                f.write_str("the risk must be above zero, and a percentage at most 100")
            }
            Self::EquityUnknown(id) => {
                write!(f, "equity is not known: {} has no price yet", id.symbol)
            }
            Self::NoEquity => f.write_str("the account has no equity to take a percentage of"),
            Self::TooLarge => f.write_str("these figures are too large to size an order from"),
        }
    }
}

impl std::error::Error for SizingError {}

/// What a trader asks to size: an order's instrument, side, and type, the
/// price it would enter at, where its stop would be, and the budget.
#[derive(Debug, Clone, PartialEq)]
pub struct SizeRequest {
    pub instrument: InstrumentId,
    pub side: Side,
    pub order_type: OrderType,
    pub entry: Decimal,
    pub stop: Decimal,
    pub budget: RiskBudget,
}

/// What a size depends on.
pub struct Inputs<'a> {
    pub side: Side,
    pub order_type: OrderType,
    pub entry: Decimal,
    pub stop: Decimal,
    pub budget: RiskBudget,
    /// The account's equity, needed only for a percentage budget.
    pub equity: Option<Result<Decimal, SizingError>>,
    pub info: Option<&'a InstrumentInfo>,
    /// How large one order could be anyway.
    pub capacity: &'a OrderCapacity,
}

pub fn size(inputs: &Inputs<'_>) -> Result<Sizing, SizingError> {
    let (entry, stop) = (inputs.entry, inputs.stop);
    if entry <= Decimal::ZERO || stop <= Decimal::ZERO {
        return Err(SizingError::PriceNotPositive);
    }
    let distance = match inputs.side {
        Side::Buy if stop < entry => entry - stop,
        Side::Sell if stop > entry => stop - entry,
        side => return Err(SizingError::StopOnWrongSide(side)),
    };
    let budget = budget(inputs.budget, inputs.equity.clone())?;
    let costs = inputs.info.map(|info| info.costs);
    let loss_per_unit =
        round_trip_costs(costs, inputs.side, entry, stop).and_then(|c| distance.checked_add(c));
    let loss_per_unit = loss_per_unit.ok_or(SizingError::TooLarge)?;
    let raw = budget
        .checked_div(loss_per_unit)
        .ok_or(SizingError::TooLarge)?;
    let sized = capacity::to_market(raw, inputs.info, inputs.order_type);
    let (quantity, capped_by) = match inputs.capacity.max_quantity {
        Some(max) if max < sized => (max, inputs.capacity.bound),
        _ => (sized, None),
    };
    let at_risk = quantity
        .checked_mul(loss_per_unit)
        .ok_or(SizingError::TooLarge)?;
    Ok(Sizing {
        quantity,
        loss_per_unit,
        budget,
        at_risk,
        capped_by,
    })
}

fn budget(
    budget: RiskBudget,
    equity: Option<Result<Decimal, SizingError>>,
) -> Result<Decimal, SizingError> {
    match budget {
        RiskBudget::Amount(amount) if amount > Decimal::ZERO => Ok(amount),
        RiskBudget::PercentOfEquity(percent)
            if percent > Decimal::ZERO && percent <= Decimal::ONE_HUNDRED =>
        {
            let equity = match equity {
                Some(Ok(equity)) => equity,
                Some(Err(unknown)) => return Err(unknown),
                None => return Err(SizingError::NoEquity),
            };
            if equity <= Decimal::ZERO {
                return Err(SizingError::NoEquity);
            }
            equity
                .checked_mul(percent)
                .and_then(|value| value.checked_div(Decimal::ONE_HUNDRED))
                .ok_or(SizingError::TooLarge)
        }
        _ => Err(SizingError::BudgetOutOfRange),
    }
}

/// Commission on the way in and on the way out at the stop, and the tax on
/// whichever leg sells: the exit for a buy, the entry for a sell.
fn round_trip_costs(
    costs: Option<TradingCosts>,
    side: Side,
    entry: Decimal,
    stop: Decimal,
) -> Option<Decimal> {
    let Some(costs) = costs else {
        return Some(Decimal::ZERO);
    };
    let sold_at = match side {
        Side::Buy => stop,
        Side::Sell => entry,
    };
    let commission = entry
        .checked_add(stop)?
        .checked_mul(costs.commission_rate)?;
    commission.checked_add(sold_at.checked_mul(costs.sell_tax_rate)?)
}

/// Cash plus every position at its mark. A position with no mark leaves
/// the equity unknown, and the error names it.
pub fn equity(
    account: &AccountState,
    mark: impl Fn(&InstrumentId) -> Option<Decimal>,
) -> Result<Decimal, SizingError> {
    account
        .positions
        .iter()
        .filter(|position| position.quantity != Decimal::ZERO)
        .try_fold(account.cash, |total, position| {
            let price = mark(&position.instrument)
                .ok_or_else(|| SizingError::EquityUnknown(position.instrument.clone()))?;
            position
                .quantity
                .checked_mul(price)
                .and_then(|value| total.checked_add(value))
                .ok_or(SizingError::TooLarge)
        })
}

#[cfg(test)]
mod tests {
    use model::{Position, TickTable};

    use super::*;

    fn d(raw: &str) -> Decimal {
        raw.parse().expect("test decimal")
    }

    fn id() -> InstrumentId {
        InstrumentId::new("sim", "AAA")
    }

    fn lots_of_100() -> InstrumentInfo {
        InstrumentInfo::new(id(), d("100"), TickTable::flat(d("0.01")).expect("tick"))
            .with_odd_lot_types([OrderType::LIMIT])
    }

    fn roomy() -> OrderCapacity {
        OrderCapacity {
            max_quantity: Some(d("1000000")),
            bound: Some(Bound::OrderSize),
            buying_power: capacity::BuyingPower {
                amount: d("1000000000"),
                estimated: false,
            },
            sellable: None,
            valued_at: None,
        }
    }

    fn sized(
        side: Side,
        entry: &str,
        stop: &str,
        budget: RiskBudget,
    ) -> Result<Sizing, SizingError> {
        size(&Inputs {
            side,
            order_type: OrderType::LIMIT,
            entry: d(entry),
            stop: d(stop),
            budget,
            equity: Some(Ok(d("100000"))),
            info: None,
            capacity: &roomy(),
        })
    }

    #[test]
    fn the_budget_over_the_distance_to_the_stop_is_the_size() {
        let sizing = sized(Side::Buy, "50", "48", RiskBudget::Amount(d("1000"))).unwrap();

        assert_eq!(sizing.quantity, d("500"));
        assert_eq!(sizing.loss_per_unit, d("2"));
        assert_eq!(sizing.at_risk, d("1000"));
        assert_eq!(sizing.capped_by, None);
    }

    #[test]
    fn a_sell_is_sized_from_a_stop_above_it() {
        let sizing = sized(Side::Sell, "50", "52.5", RiskBudget::Amount(d("1000"))).unwrap();

        assert_eq!(sizing.quantity, d("400"));
    }

    #[test]
    fn a_percentage_is_of_the_accounts_equity() {
        let sizing = sized(Side::Buy, "50", "48", RiskBudget::PercentOfEquity(d("1"))).unwrap();

        assert_eq!((sizing.budget, sizing.quantity), (d("1000"), d("500")));
    }

    #[test]
    fn a_stop_on_the_wrong_side_or_a_bad_budget_is_refused() {
        assert_eq!(
            sized(Side::Buy, "50", "50", RiskBudget::Amount(d("1000"))),
            Err(SizingError::StopOnWrongSide(Side::Buy))
        );
        assert_eq!(
            sized(Side::Sell, "50", "49", RiskBudget::Amount(d("1000"))),
            Err(SizingError::StopOnWrongSide(Side::Sell))
        );
        assert_eq!(
            sized(Side::Buy, "50", "0", RiskBudget::Amount(d("1000"))),
            Err(SizingError::PriceNotPositive)
        );
        for budget in [
            RiskBudget::Amount(d("0")),
            RiskBudget::PercentOfEquity(d("0")),
            RiskBudget::PercentOfEquity(d("100.01")),
        ] {
            assert_eq!(
                sized(Side::Buy, "50", "48", budget),
                Err(SizingError::BudgetOutOfRange)
            );
        }
    }

    #[test]
    fn costs_on_both_legs_and_the_tax_on_the_sale_come_out_of_the_budget() {
        // 0.15% commission each way and 0.1% tax on the sale at the stop:
        // 2 + (50 + 48) × 0.0015 + 48 × 0.001 = 2.195 a share.
        let info = lots_of_100().with_costs(TradingCosts::new(d("0.0015"), d("0.001")));
        let sizing = size(&Inputs {
            side: Side::Buy,
            order_type: OrderType::LIMIT,
            entry: d("50"),
            stop: d("48"),
            budget: RiskBudget::Amount(d("1000")),
            equity: None,
            info: Some(&info),
            capacity: &roomy(),
        })
        .unwrap();

        assert_eq!(sizing.loss_per_unit, d("2.195"));
        assert_eq!(
            sizing.quantity,
            d("400"),
            "455.58 rounds down to whole lots"
        );
        assert!(sizing.at_risk <= sizing.budget);
    }

    #[test]
    fn less_than_a_lot_is_an_odd_lot_where_the_market_takes_one() {
        let info = lots_of_100();
        let room = roomy();
        let inputs = |order_type| Inputs {
            side: Side::Buy,
            order_type,
            entry: d("50"),
            stop: d("48"),
            budget: RiskBudget::Amount(d("100")),
            equity: None,
            info: Some(&info),
            capacity: &room,
        };

        assert_eq!(size(&inputs(OrderType::LIMIT)).unwrap().quantity, d("50"));
        assert_eq!(size(&inputs(OrderType::MARKET)).unwrap().quantity, d("0"));
    }

    #[test]
    fn the_account_caps_what_the_budget_allows_and_says_so() {
        let cramped = OrderCapacity {
            max_quantity: Some(d("200")),
            bound: Some(Bound::BuyingPower),
            ..roomy()
        };
        let sizing = size(&Inputs {
            side: Side::Buy,
            order_type: OrderType::LIMIT,
            entry: d("50"),
            stop: d("48"),
            budget: RiskBudget::Amount(d("1000")),
            equity: None,
            info: None,
            capacity: &cramped,
        })
        .unwrap();

        assert_eq!(sizing.quantity, d("200"));
        assert_eq!(sizing.capped_by, Some(Bound::BuyingPower));
        assert_eq!(sizing.at_risk, d("400"));
    }

    #[test]
    fn equity_is_cash_and_positions_at_their_marks() {
        let account = AccountState::new(d("1000"), vec![Position::new(id(), d("10"), d("40"))]);

        assert_eq!(equity(&account, |_| Some(d("50"))), Ok(d("1500")));
        assert_eq!(
            equity(&account, |_| None),
            Err(SizingError::EquityUnknown(id()))
        );
        assert_eq!(
            equity(&account, |_| Some(Decimal::MAX)),
            Err(SizingError::TooLarge),
            "a value past what a Decimal holds is not dropped"
        );
        assert_eq!(
            sized(Side::Buy, "50", "48", RiskBudget::PercentOfEquity(d("1"))).map(|s| s.budget),
            Ok(d("1000"))
        );
    }

    #[test]
    fn a_percentage_without_a_known_equity_says_which_price_is_missing() {
        let inputs = Inputs {
            side: Side::Buy,
            order_type: OrderType::LIMIT,
            entry: d("50"),
            stop: d("48"),
            budget: RiskBudget::PercentOfEquity(d("1")),
            equity: Some(Err(SizingError::EquityUnknown(id()))),
            info: None,
            capacity: &roomy(),
        };

        assert_eq!(size(&inputs), Err(SizingError::EquityUnknown(id())));
    }
}
