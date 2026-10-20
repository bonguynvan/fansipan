//! The instrument's own status against an order: a halted or
//! suspended instrument the venue takes no orders for, and a short sale
//! under a short-sale restriction.

use model::{Decimal, InstrumentStatus, NewOrder, Position, Side};

use super::RiskReject;

/// An order against its instrument's status as the venue last published
/// it. No status means the instrument trades as its market does. `working`
/// is what working sell orders already take of the holding.
///
/// Under a short-sale restriction, a sell beyond the long holding left
/// after working sells is a short sale, and it must carry a limit price:
/// the venue's price test (above the best bid, typically) needs one, and
/// the venue applies the test itself.
pub fn check_instrument_status(
    order: &NewOrder,
    status: Option<&InstrumentStatus>,
    position: Option<&Position>,
    working: Decimal,
) -> Result<(), RiskReject> {
    let Some(status) = status else {
        return Ok(());
    };
    if !status.takes_new_orders() {
        return Err(RiskReject::NotTrading {
            status: status.trading,
            reason: status.reason.clone(),
        });
    }
    if !status.short_sale_restricted || order.side != Side::Sell || order.limit_price.is_some() {
        return Ok(());
    }
    let long = position.map_or(Decimal::ZERO, |p| p.quantity.max(Decimal::ZERO));
    // Saturating: sizes from outside cannot be trusted to subtract, and
    // nothing left to sell is the safe answer.
    let left = long
        .checked_sub(working)
        .unwrap_or(Decimal::ZERO)
        .max(Decimal::ZERO);
    if order.quantity <= left {
        return Ok(());
    }
    Err(RiskReject::ShortSaleRestricted { long: left })
}

#[cfg(test)]
mod tests {
    use super::*;
    use model::{InstrumentId, OrderType, TradingStatus, UnixNanos};

    fn d(raw: &str) -> Decimal {
        raw.parse().expect("test decimal")
    }

    fn aaa() -> InstrumentId {
        InstrumentId::new("sim", "AAA")
    }

    fn status(trading: TradingStatus) -> InstrumentStatus {
        InstrumentStatus::new(aaa(), trading, UnixNanos(1))
    }

    fn restricted() -> InstrumentStatus {
        status(TradingStatus::Trading).with_short_sale_restricted(true)
    }

    fn market_sell(quantity: &str) -> NewOrder {
        NewOrder::new(aaa(), Side::Sell, d(quantity), OrderType::MARKET, None)
    }

    fn holding(quantity: &str) -> Position {
        Position::new(aaa(), d(quantity), d("10"))
    }

    #[test]
    fn an_instrument_with_no_status_or_trading_takes_any_order() {
        assert_eq!(
            check_instrument_status(&market_sell("5"), None, None, d("0")),
            Ok(())
        );
        let trading = status(TradingStatus::Trading);
        assert_eq!(
            check_instrument_status(&market_sell("5"), Some(&trading), None, d("0")),
            Ok(())
        );
    }

    #[test]
    fn a_halted_or_suspended_instrument_takes_no_order_unless_the_venue_queues_them() {
        for trading in [TradingStatus::Halted, TradingStatus::Suspended] {
            let stopped = status(trading).with_reason("news pending");
            assert_eq!(
                check_instrument_status(&market_sell("5"), Some(&stopped), None, d("0")),
                Err(RiskReject::NotTrading {
                    status: trading,
                    reason: Some("news pending".into()),
                })
            );
            let queued = status(trading).taking_orders();
            assert_eq!(
                check_instrument_status(&market_sell("5"), Some(&queued), None, d("0")),
                Ok(())
            );
        }
    }

    #[test]
    fn under_a_short_sale_restriction_a_short_sale_needs_a_limit_price() {
        let restricted = restricted();
        let held = holding("10");

        let short =
            check_instrument_status(&market_sell("11"), Some(&restricted), Some(&held), d("0"));
        let priced = NewOrder::limit(aaa(), Side::Sell, d("11"), d("9"));

        assert_eq!(
            short,
            Err(RiskReject::ShortSaleRestricted { long: d("10") })
        );
        assert_eq!(
            check_instrument_status(&priced, Some(&restricted), Some(&held), d("0")),
            Ok(())
        );
    }

    #[test]
    fn under_a_short_sale_restriction_selling_the_holding_and_buying_are_free() {
        let restricted = restricted();
        let held = holding("10");
        let buy = NewOrder::new(aaa(), Side::Buy, d("50"), OrderType::MARKET, None);

        assert_eq!(
            check_instrument_status(&market_sell("10"), Some(&restricted), Some(&held), d("0")),
            Ok(())
        );
        assert_eq!(
            check_instrument_status(&buy, Some(&restricted), None, d("0")),
            Ok(())
        );
    }

    #[test]
    fn working_sells_and_a_short_position_leave_less_to_sell_unrestricted() {
        let restricted = restricted();

        assert_eq!(
            check_instrument_status(
                &market_sell("5"),
                Some(&restricted),
                Some(&holding("10")),
                d("6")
            ),
            Err(RiskReject::ShortSaleRestricted { long: d("4") })
        );
        assert_eq!(
            check_instrument_status(
                &market_sell("1"),
                Some(&restricted),
                Some(&holding("-3")),
                d("0")
            ),
            Err(RiskReject::ShortSaleRestricted { long: d("0") })
        );
    }
}
