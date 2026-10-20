//! How large one order could be, before the trader types its size: what the
//! account can pay for or holds, and this terminal's own limits, shaped to
//! the quantities the market takes. Risk still judges the order
//! itself; this says where its limits lie, using the same rules.

use model::{AccountState, Decimal, InstrumentInfo, OrderType, Position, PriceBand, Side};

use crate::risk::{self, RiskLimits};

/// What sets the largest order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bound {
    /// What the account can spend on buys.
    BuyingPower,
    /// The settled shares not already in working sells, where the market
    /// forbids short selling.
    Holding,
    /// This terminal's cap on one order's quantity.
    OrderSize,
    /// This terminal's cap on one order's value.
    OrderValue,
}

/// What the account can spend on buys.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BuyingPower {
    pub amount: Decimal,
    /// Core's estimate from cash less working buys, because the venue
    /// reported no figure of its own.
    pub estimated: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OrderCapacity {
    /// The largest quantity one order may have: whole lots, or an odd lot
    /// when less than a lot fits and the market takes this type as one.
    /// `None` for a buy with no price to value it at.
    pub max_quantity: Option<Decimal>,
    /// What sets `max_quantity`.
    pub bound: Option<Bound>,
    pub buying_power: BuyingPower,
    /// What a sell may take now where the market forbids short selling;
    /// `None` where it allows it.
    pub sellable: Option<Decimal>,
    /// The price a buy is valued at: its limit, or the day's ceiling for an
    /// order without one.
    pub valued_at: Option<Decimal>,
}

/// What an order's capacity depends on.
pub struct Inputs<'a> {
    pub side: Side,
    pub order_type: OrderType,
    pub limit_price: Option<Decimal>,
    pub info: Option<&'a InstrumentInfo>,
    pub band: Option<&'a PriceBand>,
    pub account: &'a AccountState,
    pub position: Option<&'a Position>,
    /// What this session's working buys in every instrument may still
    /// spend, for an estimate when the venue gives no buying power.
    pub working_buys: Decimal,
    /// What this session's working sells in this instrument may still sell.
    pub working_sells: Decimal,
    pub limits: &'a RiskLimits,
}

pub fn capacity(inputs: &Inputs<'_>) -> OrderCapacity {
    let buying_power = buying_power(inputs.account, inputs.working_buys);
    let sellable = forbids_short(inputs.info)
        .then(|| risk::sellable_now(inputs.position, inputs.working_sells));
    let priced = inputs.limit_price.filter(|price| *price > Decimal::ZERO);
    let valued_at = match inputs.side {
        Side::Buy => priced.or_else(|| inputs.band.and_then(|band| band.ceiling)),
        Side::Sell => priced,
    }
    .filter(|price| *price > Decimal::ZERO);

    // In order of how much they tell the trader: their own money or shares
    // first, then this terminal's caps.
    let own = match inputs.side {
        Side::Buy => valued_at
            .and_then(|price| buying_power.amount.checked_div(price))
            .map(|quantity| (quantity, Bound::BuyingPower)),
        Side::Sell => sellable.map(|quantity| (quantity, Bound::Holding)),
    };
    // Risk caps a priced order's value only; so does this.
    let value_cap = priced
        .and_then(|price| inputs.limits.max_order_notional.checked_div(price))
        .map(|quantity| (quantity, Bound::OrderValue));
    let size_cap = Some((inputs.limits.max_order_quantity, Bound::OrderSize));

    let unpriceable_buy = inputs.side == Side::Buy && valued_at.is_none();
    let tightest = [own, size_cap, value_cap]
        .into_iter()
        .flatten()
        .reduce(|tightest, next| if next.0 < tightest.0 { next } else { tightest });
    let (max_quantity, bound) = match tightest {
        Some((raw, bound)) if !unpriceable_buy => (
            Some(to_market(raw, inputs.info, inputs.order_type)),
            Some(bound),
        ),
        _ => (None, None),
    };

    OrderCapacity {
        max_quantity,
        bound,
        buying_power,
        sellable,
        valued_at,
    }
}

/// The venue's figure when it gives one; otherwise cash less what working
/// buys may still spend. Never below zero.
fn buying_power(account: &AccountState, working_buys: Decimal) -> BuyingPower {
    let (amount, estimated) = match account.buying_power {
        Some(amount) => (amount, false),
        None => (
            account
                .cash
                .checked_sub(working_buys)
                .unwrap_or(Decimal::ZERO),
            true,
        ),
    };
    BuyingPower {
        amount: amount.max(Decimal::ZERO),
        estimated,
    }
}

fn forbids_short(info: Option<&InstrumentInfo>) -> bool {
    info.is_some_and(|info| !info.short_selling)
}

/// The largest quantity at most `raw` that the market takes in one order of
/// `order_type`: whole lots; or, when not one lot fits, an odd lot if the
/// market takes this type as one. Whole units where no lot is set.
pub(crate) fn to_market(
    raw: Decimal,
    info: Option<&InstrumentInfo>,
    order_type: OrderType,
) -> Decimal {
    let raw = raw.max(Decimal::ZERO);
    let units = raw.floor();
    let Some(info) = info.filter(|info| info.lot_size > Decimal::ZERO) else {
        return units;
    };
    let lots = raw
        .checked_div(info.lot_size)
        .and_then(|lots| lots.floor().checked_mul(info.lot_size))
        .unwrap_or(Decimal::ZERO);
    if lots > Decimal::ZERO {
        lots
    } else if info.odd_lot_types.contains(&order_type) && units < info.lot_size {
        units
    } else {
        Decimal::ZERO
    }
}

#[cfg(test)]
mod tests {
    use model::{InstrumentId, NewOrder, OrderKind, TickTable, TimeInForce};

    use super::*;

    fn d(raw: &str) -> Decimal {
        raw.parse().expect("test decimal")
    }

    fn vnm() -> InstrumentId {
        InstrumentId::new("sim", "VNM")
    }

    /// Vietnam's rules: lots of 100, odd lots as limit orders for the day,
    /// no short selling, a 25,000 reference with a 7% band.
    fn vn_info() -> InstrumentInfo {
        InstrumentInfo::new(vnm(), d("100"), TickTable::flat(d("50")).expect("tick"))
            .with_short_selling(false)
            .with_odd_lot_types([OrderType::LIMIT])
    }

    fn band() -> PriceBand {
        PriceBand::new(
            vnm(),
            d("25000"),
            Some(d("26750")),
            Some(d("23250")),
            model::UnixNanos(0),
        )
    }

    const LIMITS: RiskLimits = RiskLimits {
        max_order_quantity: Decimal::from_parts(1_000_000, 0, 0, false, 0),
        max_order_notional: Decimal::from_parts(1_000_000_000, 0, 0, false, 0),
    };

    struct Case {
        side: Side,
        order_type: OrderType,
        limit: Option<&'static str>,
        account: AccountState,
        position: Option<Position>,
        working_buys: &'static str,
        working_sells: &'static str,
        limits: RiskLimits,
        info: Option<InstrumentInfo>,
    }

    fn buy(limit: Option<&'static str>, account: AccountState) -> Case {
        Case {
            side: Side::Buy,
            order_type: if limit.is_some() {
                OrderType::LIMIT
            } else {
                OrderType::MARKET
            },
            limit,
            account,
            position: None,
            working_buys: "0",
            working_sells: "0",
            limits: LIMITS,
            info: Some(vn_info()),
        }
    }

    fn sell(held: &str, settled: &str, working: &'static str) -> Case {
        Case {
            side: Side::Sell,
            order_type: OrderType::LIMIT,
            limit: Some("25000"),
            account: AccountState::new(d("0"), vec![]),
            position: Some(Position::new(vnm(), d(held), d("24000")).with_settled(d(settled))),
            working_buys: "0",
            working_sells: working,
            limits: LIMITS,
            info: Some(vn_info()),
        }
    }

    fn run(case: &Case) -> OrderCapacity {
        let band = band();
        capacity(&Inputs {
            side: case.side,
            order_type: case.order_type,
            limit_price: case.limit.map(d),
            info: case.info.as_ref(),
            band: Some(&band),
            account: &case.account,
            position: case.position.as_ref(),
            working_buys: d(case.working_buys),
            working_sells: d(case.working_sells),
            limits: &case.limits,
        })
    }

    fn cash(amount: &str) -> AccountState {
        AccountState::new(d(amount), vec![])
    }

    #[test]
    fn a_buy_takes_the_venues_buying_power_in_whole_lots() {
        let capacity = run(&buy(
            Some("25000"),
            cash("1").with_buying_power(d("9000000")),
        ));

        // 9,000,000 / 25,000 = 360, so three whole lots.
        assert_eq!(capacity.max_quantity, Some(d("300")));
        assert_eq!(capacity.bound, Some(Bound::BuyingPower));
        assert_eq!(
            capacity.buying_power,
            BuyingPower {
                amount: d("9000000"),
                estimated: false
            }
        );
    }

    #[test]
    fn less_than_a_lot_is_an_odd_lot_where_the_market_takes_one() {
        let market = buy(None, cash("2000000"));
        let limit = buy(Some("25000"), cash("2000000"));

        assert_eq!(run(&limit).max_quantity, Some(d("80")));
        // A market order is valued at the ceiling, and is no odd lot here.
        assert_eq!(run(&market).max_quantity, Some(d("0")));
        assert_eq!(run(&market).valued_at, Some(d("26750")));
    }

    #[test]
    fn without_the_venues_figure_buying_power_is_estimated_from_cash() {
        let mut case = buy(Some("25000"), cash("10000000"));
        case.working_buys = "2500000";

        let capacity = run(&case);

        assert_eq!(capacity.max_quantity, Some(d("300")));
        assert_eq!(
            capacity.buying_power,
            BuyingPower {
                amount: d("7500000"),
                estimated: true
            }
        );
    }

    #[test]
    fn a_buy_with_nothing_to_value_it_at_has_no_largest_size() {
        let case = buy(None, cash("10000000"));
        let unbounded = capacity(&Inputs {
            side: case.side,
            order_type: case.order_type,
            limit_price: None,
            info: case.info.as_ref(),
            band: None,
            account: &case.account,
            position: None,
            working_buys: Decimal::ZERO,
            working_sells: Decimal::ZERO,
            limits: &case.limits,
        });

        assert_eq!(unbounded.max_quantity, None);
        assert_eq!(unbounded.bound, None);
        assert_eq!(unbounded.buying_power.amount, d("10000000"));
    }

    #[test]
    fn a_sell_takes_the_settled_shares_not_in_working_sells() {
        let capacity = run(&sell("500", "300", "50"));

        // 300 settled, 50 already selling: 250 may go, so two whole lots.
        assert_eq!(capacity.sellable, Some(d("250")));
        assert_eq!(capacity.max_quantity, Some(d("200")));
        assert_eq!(capacity.bound, Some(Bound::Holding));
        assert_eq!(run(&sell("500", "60", "0")).max_quantity, Some(d("60")));
        assert_eq!(run(&sell("0", "0", "0")).max_quantity, Some(d("0")));
    }

    #[test]
    fn where_short_selling_is_allowed_only_the_terminals_caps_bound_a_sell() {
        let mut case = sell("0", "0", "0");
        case.info = Some(vn_info().with_short_selling(true));
        case.limits = RiskLimits {
            max_order_quantity: d("5000"),
            ..LIMITS
        };

        let capacity = run(&case);

        assert_eq!(capacity.sellable, None);
        assert_eq!(capacity.max_quantity, Some(d("5000")));
        assert_eq!(capacity.bound, Some(Bound::OrderSize));
    }

    #[test]
    fn the_terminals_value_cap_can_be_the_tighter_one() {
        let mut case = buy(Some("25000"), cash("1").with_buying_power(d("900000000")));
        case.limits = RiskLimits {
            max_order_notional: d("100000000"),
            ..LIMITS
        };

        let capacity = run(&case);

        assert_eq!(capacity.max_quantity, Some(d("4000")));
        assert_eq!(capacity.bound, Some(Bound::OrderValue));
    }

    #[test]
    fn absurd_figures_give_a_size_not_a_panic() {
        let mut tiny = buy(
            Some("0.0000000000000000000000000001"),
            cash("1").with_buying_power(Decimal::MAX),
        );
        tiny.info = None;
        let negative = buy(Some("25000"), cash("1").with_buying_power(d("-5")));

        assert!(run(&tiny).max_quantity.is_some());
        assert_eq!(run(&negative).max_quantity, Some(d("0")));
        assert_eq!(run(&negative).buying_power.amount, d("0"));
    }

    #[test]
    fn the_largest_size_is_one_risk_accepts() {
        let cases = [
            buy(Some("25000"), cash("1").with_buying_power(d("9000000"))),
            buy(Some("25000"), cash("2000000")),
            sell("500", "300", "50"),
            sell("500", "60", "0"),
        ];
        for case in &cases {
            let max = run(case).max_quantity.expect("a size");
            let order = NewOrder::new(vnm(), case.side, max, case.order_type, case.limit.map(d));
            let info = case.info.as_ref();

            assert_eq!(risk::check(&order, &case.limits), Ok(()));
            assert_eq!(
                risk::check_market_rules(&order, info, Some(&band())),
                Ok(())
            );
            assert_eq!(
                risk::check_sellable(&order, info, case.position.as_ref(), d(case.working_sells)),
                Ok(())
            );
        }
    }

    #[test]
    fn an_odd_lot_needs_a_type_the_market_takes_as_one() {
        let ioc = OrderType::new(OrderKind::Limit, TimeInForce::ImmediateOrCancel);
        let mut case = buy(Some("25000"), cash("2000000"));
        case.order_type = ioc;

        assert_eq!(run(&case).max_quantity, Some(d("0")));
    }
}
