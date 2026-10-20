//! Properties of the order lifecycle: whatever a venue
//! reports, in whatever order, late, duplicated, or nonsensical, an order
//! keeps the invariants below, and nothing panics. In the engine, a report
//! it refuses leaves the order exactly as it was.
//!
//! The seed is fixed, so every run explores the same cases (tests
//! are deterministic). Set PROPTEST_RNG_SEED to explore others.

use std::collections::HashSet;

use adapter_api::{AdapterError, ExecutionClient};
use bus::BusHandle;
use fansipan_execution_engine::{lifecycle, ExecutionEngine, RiskLimits};
use model::{
    AccountState, CancelOrder, ClientOrderId, Decimal, ExecutionReport, FillId, InstrumentId,
    NewOrder, Order, OrderEvent, OrderStatus, OrderType, Side, SubmitOrder, UnixNanos,
    VenueOrderId,
};
use proptest::collection::vec;
use proptest::prelude::*;
use proptest::test_runner::RngSeed;

const SEED: u64 = 20_261_008;

fn config() -> ProptestConfig {
    let config = ProptestConfig::default();
    if std::env::var_os("PROPTEST_RNG_SEED").is_some() {
        return config;
    }
    ProptestConfig {
        rng_seed: RngSeed::Fixed(SEED),
        ..config
    }
}

/// An order in any state the lifecycle can find it in, consistent with
/// itself: nothing filled before it rests, part filled when partly filled,
/// and a venue id once the venue acknowledged it (a fill may beat the ack).
fn any_open_order() -> impl Strategy<Value = Order> {
    let statuses = prop_oneof![
        Just(OrderStatus::Submitted),
        Just(OrderStatus::Accepted),
        Just(OrderStatus::PartiallyFilled),
        Just(OrderStatus::PendingCancel),
        Just(OrderStatus::PendingAmend),
    ];
    // Small sizes and fills are common, so the venue's reports below often
    // name exactly what has filled, where the lifecycle has special cases.
    let small_or_any = || prop_oneof![2 => 0u32..12, 1 => 0u32..100];
    (2u32..100, statuses, any::<bool>(), small_or_any()).prop_map(
        |(quantity, status, acked, part)| {
            let filled = match status {
                OrderStatus::Submitted | OrderStatus::Accepted => 0,
                OrderStatus::PartiallyFilled => 1 + part % (quantity - 1),
                _ => part % quantity,
            };
            let venue_order_id = (status == OrderStatus::Accepted
                || (acked && status != OrderStatus::Submitted))
                .then(|| VenueOrderId("V0".into()));
            Order {
                id: ClientOrderId(1),
                venue_order_id,
                instrument: InstrumentId::new("sim", "AAA"),
                side: Side::Buy,
                quantity: Decimal::from(quantity),
                order_type: OrderType::LIMIT,
                limit_price: Some(Decimal::new(2500, 2)),
                status,
                filled_quantity: Decimal::from(filled),
                avg_fill_price: (filled > 0).then(|| Decimal::new(2500, 2)),
                reason: None,
            }
        },
    )
}

/// A count below `limit`, often zero or small: zero, and sizes equal to
/// what has filled, are where the lifecycle has its special cases.
fn edgy(limit: u32) -> impl Strategy<Value = u32> {
    prop_oneof![1 => Just(0u32), 2 => 0u32..12, 3 => 0..limit]
}

/// Any report a venue could send, including ones that make no sense now:
/// zero quantities, prices of zero, fills bigger than the order,
/// amendments to zero or below what has filled.
fn event() -> impl Strategy<Value = OrderEvent> {
    prop_oneof![
        (0u8..3).prop_map(|v| OrderEvent::Accepted {
            venue_order_id: VenueOrderId(format!("V{v}")),
        }),
        Just(OrderEvent::Rejected {
            reason: "refused".into()
        }),
        (0u8..6, edgy(120), edgy(5_000)).prop_map(|(f, quantity, cents)| OrderEvent::Filled {
            fill_id: FillId(format!("F{f}")),
            quantity: Decimal::from(quantity),
            price: Decimal::new(i64::from(cents), 2),
        }),
        Just(OrderEvent::Cancelled),
        Just(OrderEvent::CancelRejected {
            reason: "too late".into()
        }),
        (edgy(150), 1i64..5_000).prop_map(|(quantity, cents)| OrderEvent::Amended {
            quantity: Decimal::from(quantity),
            limit_price: Some(Decimal::new(cents, 2)),
        }),
        Just(OrderEvent::AmendRejected {
            reason: "refused".into()
        }),
    ]
}

/// What an order's status says about what has filled.
fn status_matches_fills(order: &Order) -> bool {
    let filled = order.filled_quantity;
    let none = filled.is_zero();
    let some_left = filled < order.quantity;
    match order.status {
        OrderStatus::Submitted | OrderStatus::Rejected | OrderStatus::Denied => none,
        OrderStatus::Accepted => none && order.venue_order_id.is_some(),
        OrderStatus::PartiallyFilled => !none && some_left,
        OrderStatus::Filled => !none && filled == order.quantity,
        OrderStatus::PendingCancel | OrderStatus::PendingAmend => some_left,
        // Cancelled with something left, or with nothing traded at all (a
        // venue amending an order down to zero).
        OrderStatus::Cancelled => some_left || none,
        // A status added later must say here what it means for fills.
        _ => false,
    }
}

proptest! {
    #![proptest_config(config())]

    #[test]
    fn an_order_keeps_its_invariants_whatever_the_venue_reports(
        start in any_open_order(),
        events in vec(event(), 0..40),
    ) {
        let mut order = start;
        let mut fill_prices: Vec<Decimal> = order.avg_fill_price.into_iter().collect();
        let mut finished = false;
        let mut venue_id = order.venue_order_id.clone();

        for event in &events {
            let before = order.clone();
            if let Ok(next) = lifecycle::apply(&order, event) {
                if let OrderEvent::Filled { price, .. } = event {
                    if next.filled_quantity != before.filled_quantity {
                        fill_prices.push(*price);
                    }
                }
                order = next;
            }

            // Filled never exceeds ordered, and never goes negative.
            prop_assert!(order.filled_quantity >= Decimal::ZERO);
            prop_assert!(order.filled_quantity <= order.quantity);
            // The status agrees with what has filled.
            prop_assert!(status_matches_fills(&order), "{:?} with {} of {} filled", order.status, order.filled_quantity, order.quantity);
            // An average price exactly when something filled, inside the
            // range of the prices that filled.
            prop_assert_eq!(order.avg_fill_price.is_some(), !order.filled_quantity.is_zero());
            if let Some(average) = order.avg_fill_price {
                let lowest = fill_prices.iter().min().copied().unwrap_or(average);
                let highest = fill_prices.iter().max().copied().unwrap_or(average);
                prop_assert!(lowest <= average && average <= highest, "{average} outside {lowest}..={highest}");
            }
            // A finished order never works again; a late fill may complete a
            // cancelled one, which is still finished.
            if finished {
                prop_assert!(!order.status.is_open(), "{:?} reopened", order.status);
            }
            finished |= !order.status.is_open();
            // The first venue id the order learns is the one it keeps.
            if let Some(id) = &venue_id {
                prop_assert_eq!(order.venue_order_id.as_ref(), Some(id));
            }
            venue_id = order.venue_order_id.clone();
        }
    }
}

proptest! {
    #![proptest_config(config())]

    /// An amendment to just what has filled, or a little more, from any
    /// state: terms that leave nothing to trade end the order, a cancel
    /// pending or not, and every result agrees with what has filled.
    #[test]
    fn terms_that_leave_nothing_to_trade_end_the_order(
        start in any_open_order(),
        extra in 0u32..3,
        cents in 1i64..5_000,
    ) {
        let amended = OrderEvent::Amended {
            quantity: start.filled_quantity + Decimal::from(extra),
            limit_price: Some(Decimal::new(cents, 2)),
        };

        match lifecycle::apply(&start, &amended) {
            Ok(order) => {
                prop_assert!(status_matches_fills(&order), "{:?} with {} of {} filled", order.status, order.filled_quantity, order.quantity);
                if extra == 0 {
                    prop_assert!(!order.status.is_open(), "{:?} still open with nothing to trade", order.status);
                }
            }
            // Only an order the venue has not acknowledged takes no terms.
            Err(_) => prop_assert_eq!(start.status, OrderStatus::Submitted),
        }
    }
}

/// An execution client that takes every request and does nothing with it.
struct Silent;

impl ExecutionClient for Silent {
    fn connect(&mut self, _bus: Box<dyn BusHandle>) -> Result<(), AdapterError> {
        Ok(())
    }
    fn submit_order(&mut self, _order: SubmitOrder) -> Result<(), AdapterError> {
        Ok(())
    }
    fn cancel_order(&mut self, _cancel: CancelOrder) -> Result<(), AdapterError> {
        Ok(())
    }
    fn account_state(&mut self) -> Result<AccountState, AdapterError> {
        Ok(AccountState::default())
    }
    fn disconnect(&mut self) -> Result<(), AdapterError> {
        Ok(())
    }
}

/// An engine holding one working buy of `quantity` at 5.
fn engine_with_order(quantity: u32) -> (ExecutionEngine, ClientOrderId) {
    let mut engine = ExecutionEngine::new(
        Box::new(Silent),
        RiskLimits {
            max_order_quantity: Decimal::from(100_000),
            max_order_notional: Decimal::from(100_000_000),
        },
    );
    let order = engine.submit(NewOrder::limit(
        InstrumentId::new("sim", "AAA"),
        Side::Buy,
        Decimal::from(quantity),
        Decimal::from(5),
    ));
    assert!(order.status.is_open(), "the order passes risk");
    (engine, order.id)
}

fn report(order_id: ClientOrderId, event: OrderEvent) -> ExecutionReport {
    ExecutionReport {
        order_id,
        event,
        ts_event: UnixNanos(0),
    }
}

const ORDER_QUANTITY: u32 = 1_000;

/// One fill per id, as a venue sends it: the same id always carries the
/// same quantity, however often it is reported.
fn fill_of(id: u8) -> (FillId, Decimal) {
    (
        FillId(format!("F{id}")),
        Decimal::from(u32::from(id) * 37 + 11),
    )
}

proptest! {
    #![proptest_config(config())]

    #[test]
    fn a_fill_reported_twice_counts_once(ids in vec(0u8..12, 1..40)) {
        let (mut engine, id) = engine_with_order(ORDER_QUANTITY);

        let mut counted = HashSet::new();
        let mut expected = Decimal::ZERO;
        for fill in ids {
            let (fill_id, quantity) = fill_of(fill);
            let applied = engine.apply(&report(id, OrderEvent::Filled {
                fill_id,
                quantity,
                price: Decimal::from(5),
            }));
            // A new fill that fits counts; a repeat, or one that would
            // overfill, does not.
            if !counted.contains(&fill) && expected + quantity <= Decimal::from(ORDER_QUANTITY) {
                prop_assert!(applied.is_ok());
                counted.insert(fill);
                expected += quantity;
            }
            let filled = engine.order(id).map(|o| o.filled_quantity);
            prop_assert_eq!(filled, Some(expected));
        }
    }

    #[test]
    fn a_report_the_engine_refuses_leaves_the_order_as_it_was(
        quantity in 1u32..200,
        events in vec(event(), 1..40),
    ) {
        let (mut engine, id) = engine_with_order(quantity);
        for event in events {
            let before = engine.order(id).cloned();
            if engine.apply(&report(id, event)).is_err() {
                prop_assert_eq!(engine.order(id).cloned(), before);
            }
        }
    }
}

/// A fill refused because it did not fit yet is not remembered as applied:
/// once an amendment makes room, the same fill reported again counts. A
/// fill can reach the terminal before the amendment that made room for it.
#[test]
fn a_fill_refused_for_overfilling_counts_once_it_fits() {
    let (mut engine, id) = engine_with_order(10);
    let acked = engine.apply(&report(
        id,
        OrderEvent::Accepted {
            venue_order_id: VenueOrderId("V1".into()),
        },
    ));
    let fill = || {
        report(
            id,
            OrderEvent::Filled {
                fill_id: FillId("F1".into()),
                quantity: Decimal::from(15),
                price: Decimal::from(5),
            },
        )
    };

    let too_big = engine.apply(&fill());
    let amended = engine.apply(&report(
        id,
        OrderEvent::Amended {
            quantity: Decimal::from(20),
            limit_price: Some(Decimal::from(5)),
        },
    ));
    let fits = engine.apply(&fill());

    assert!(acked.is_ok());
    assert!(too_big.is_err());
    assert!(amended.is_ok());
    assert!(fits.is_ok());
    assert_eq!(
        engine.order(id).map(|o| o.filled_quantity),
        Some(Decimal::from(15))
    );
}
