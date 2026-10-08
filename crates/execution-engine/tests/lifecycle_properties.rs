//! Properties of the order lifecycle (roadmap Phase 7): whatever a venue
//! reports, in whatever order, late, duplicated, or nonsensical, an order
//! keeps the invariants below, and nothing panics. An event the lifecycle
//! refuses leaves the order exactly as it was.

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

fn submitted(quantity: u32) -> Order {
    Order {
        id: ClientOrderId(1),
        venue_order_id: None,
        instrument: InstrumentId::new("sim", "AAA"),
        side: Side::Buy,
        quantity: Decimal::from(quantity),
        order_type: OrderType::LIMIT,
        limit_price: Some(Decimal::new(2500, 2)),
        status: OrderStatus::Submitted,
        filled_quantity: Decimal::ZERO,
        avg_fill_price: None,
        reason: None,
    }
}

/// Any report a venue could send, including ones that make no sense now:
/// zero or negative-looking quantities, prices of zero, fills bigger than
/// the order, amendments below what has filled.
fn event() -> impl Strategy<Value = OrderEvent> {
    prop_oneof![
        (0u8..3).prop_map(|v| OrderEvent::Accepted {
            venue_order_id: VenueOrderId(format!("V{v}")),
        }),
        Just(OrderEvent::Rejected {
            reason: "refused".into()
        }),
        (0u8..6, 0u32..120, 0i64..5_000).prop_map(|(f, quantity, cents)| OrderEvent::Filled {
            fill_id: FillId(format!("F{f}")),
            quantity: Decimal::from(quantity),
            price: Decimal::new(cents, 2),
        }),
        Just(OrderEvent::Cancelled),
        Just(OrderEvent::CancelRejected {
            reason: "too late".into()
        }),
        (0u32..150, 1i64..5_000).prop_map(|(quantity, cents)| OrderEvent::Amended {
            quantity: Decimal::from(quantity),
            limit_price: Some(Decimal::new(cents, 2)),
        }),
        Just(OrderEvent::AmendRejected {
            reason: "refused".into()
        }),
    ]
}

proptest! {
    #[test]
    fn an_order_keeps_its_invariants_whatever_the_venue_reports(
        quantity in 1u32..100,
        events in vec(event(), 0..40),
    ) {
        let mut order = submitted(quantity);
        let mut fill_prices: Vec<Decimal> = Vec::new();
        let mut finished = false;
        let mut venue_id: Option<VenueOrderId> = None;

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
            // A rejection erases nothing: an order with fills is never rejected.
            if order.status == OrderStatus::Rejected {
                prop_assert!(order.filled_quantity.is_zero());
            }
            // The first venue id the order learns is the one it keeps.
            if let Some(id) = &venue_id {
                prop_assert_eq!(order.venue_order_id.as_ref(), Some(id));
            }
            venue_id = order.venue_order_id.clone();
        }
    }

    #[test]
    fn a_refused_event_leaves_the_order_as_it_was(
        quantity in 1u32..100,
        events in vec(event(), 1..40),
    ) {
        let mut order = submitted(quantity);
        for event in &events {
            let before = order.clone();
            match lifecycle::apply(&order, event) {
                Ok(next) => order = next,
                Err(_) => prop_assert_eq!(&order, &before),
            }
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
    #[test]
    fn a_fill_reported_twice_counts_once(ids in vec(0u8..12, 1..40)) {
        let mut engine = ExecutionEngine::new(
            Box::new(Silent),
            RiskLimits {
                max_order_quantity: Decimal::from(10_000),
                max_order_notional: Decimal::from(10_000_000),
            },
        );
        let order = engine.submit(NewOrder::limit(
            InstrumentId::new("sim", "AAA"),
            Side::Buy,
            Decimal::from(ORDER_QUANTITY),
            Decimal::from(5),
        ));
        prop_assert!(order.status.is_open());
        let report = |event| ExecutionReport { order_id: order.id, event, ts_event: UnixNanos(0) };

        let mut counted = HashSet::new();
        let mut expected = Decimal::ZERO;
        for id in ids {
            let (fill_id, quantity) = fill_of(id);
            let applied = engine.apply(&report(OrderEvent::Filled {
                fill_id,
                quantity,
                price: Decimal::from(5),
            }));
            // A new fill that fits counts; a repeat, or one that would
            // overfill, does not.
            if !counted.contains(&id) && expected + quantity <= Decimal::from(ORDER_QUANTITY) {
                prop_assert!(applied.is_ok());
                counted.insert(id);
                expected += quantity;
            }
            let filled = engine.order(order.id).map(|o| o.filled_quantity);
            prop_assert_eq!(filled, Some(expected));
        }
    }
}
