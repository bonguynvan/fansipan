//! Properties of risk's market-rule check (roadmap Phase 7): on a market
//! with Vietnam's rules, an order passes exactly when its quantity is one
//! the market takes and its price is on the ladder and inside the day's
//! band. Nothing a venue would refuse for those reasons gets through, and
//! nothing it would take is refused.

use fansipan_execution_engine::risk::{self, RiskReject};
use model::{
    Decimal, InstrumentId, InstrumentInfo, NewOrder, OrderType, PriceBand, Side, TickTable,
    UnixNanos,
};
use proptest::prelude::*;

/// HOSE's ladder: 10 below 10,000, 50 below 50,000, 100 above.
fn hose_ticks() -> TickTable {
    TickTable::new(vec![
        (Decimal::ZERO, Decimal::from(10)),
        (Decimal::from(10_000), Decimal::from(50)),
        (Decimal::from(50_000), Decimal::from(100)),
    ])
    .expect("HOSE's ladder")
}

/// Lots of 100, odd lots as limit orders for the day.
fn info() -> InstrumentInfo {
    InstrumentInfo::new(
        InstrumentId::new("sim", "VNM"),
        Decimal::from(100),
        hose_ticks(),
    )
    .with_odd_lot_types([OrderType::LIMIT])
}

/// A 7% band around a reference on the ladder.
fn band(reference: Decimal) -> PriceBand {
    PriceBand::from_fraction(
        InstrumentId::new("sim", "VNM"),
        reference,
        Decimal::new(7, 2),
        &hose_ticks(),
        UnixNanos(0),
    )
    .expect("a valid band")
}

fn order(quantity: u32, price: Decimal, order_type: OrderType) -> NewOrder {
    NewOrder::new(
        InstrumentId::new("sim", "VNM"),
        Side::Buy,
        Decimal::from(quantity),
        order_type,
        Some(price),
    )
}

/// Whole lots, odd lots, and sizes that are neither, in equal measure.
fn quantity() -> impl Strategy<Value = u32> {
    prop_oneof![
        (1u32..20).prop_map(|lots| lots * 100),
        1u32..100,
        101u32..1_000
    ]
}

proptest! {
    #[test]
    fn an_order_passes_exactly_when_the_market_would_take_it(
        reference_units in 100u32..20_000,
        anywhere_units in 100u32..20_000,
        near_band in any::<bool>(),
        offset_permille in -150i32..150,
        quantity in quantity(),
        limit_for_the_day in any::<bool>(),
    ) {
        // Prices in tens of VND, so some fall between ticks above 10,000;
        // half of them near the reference, where the band's edges are.
        let reference = hose_ticks().round_up(Decimal::from(reference_units * 10));
        let price = if near_band {
            let moved = reference * Decimal::new(1_000 + i64::from(offset_permille), 3);
            (moved / Decimal::from(10)).round() * Decimal::from(10)
        } else {
            Decimal::from(anywhere_units * 10)
        };
        let order_type = if limit_for_the_day {
            OrderType::LIMIT
        } else {
            OrderType::new(model::OrderKind::Limit, model::TimeInForce::ImmediateOrCancel)
        };
        let info = info();
        let band = band(reference);

        let verdict = risk::check_market_rules(&order(quantity, price, order_type), Some(&info), Some(&band));

        let quantity_taken = quantity % 100 == 0 || (quantity < 100 && limit_for_the_day);
        let price_taken = info.ticks.is_on_tick(price) && band.contains(price);
        prop_assert_eq!(verdict.is_ok(), quantity_taken && price_taken, "{:?}", verdict);
        if let Err(reason) = verdict {
            let explained = match reason {
                RiskReject::NotLotMultiple { .. }
                | RiskReject::NotLotsOrOddLot { .. }
                | RiskReject::OddLotType { .. } => !quantity_taken,
                RiskReject::PriceNotOnTick { .. } => !info.ticks.is_on_tick(price),
                RiskReject::PriceAboveCeiling { .. } | RiskReject::PriceBelowFloor { .. } => {
                    !band.contains(price)
                }
                _ => false,
            };
            prop_assert!(explained, "refused for a reason that does not hold");
        }
    }
}
