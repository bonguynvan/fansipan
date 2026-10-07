//! The order state machine: one pure function from (order, venue event) to
//! the next order. The full table, with reasons, is /docs/order-lifecycle.md.

use std::fmt;

use model::{Decimal, Order, OrderEvent, OrderStatus};

#[derive(Debug, Clone, PartialEq)]
pub enum TransitionError {
    /// The event makes no sense for an order in this status, e.g. an
    /// acceptance for an order the venue already rejected.
    Invalid { from: OrderStatus, event: String },
    /// A fill larger than what is left to fill.
    Overfill { remaining: Decimal, fill: Decimal },
    /// A fill with a non-positive quantity or price.
    MalformedFill,
    /// Fill arithmetic would overflow `Decimal`: values no real venue sends.
    OutOfRange,
}

impl fmt::Display for TransitionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid { from, event } => write!(f, "{event} is not valid for a {from:?} order"),
            Self::Overfill { remaining, fill } => {
                write!(f, "fill of {fill} exceeds the {remaining} left to fill")
            }
            Self::MalformedFill => f.write_str("fill quantity and price must be positive"),
            Self::OutOfRange => f.write_str("fill values are out of range"),
        }
    }
}

impl std::error::Error for TransitionError {}

/// Returns the order as it stands after `event`. The input is never modified,
/// so a rejected transition leaves the caller's state exactly as it was.
pub fn apply(order: &Order, event: &OrderEvent) -> Result<Order, TransitionError> {
    use OrderStatus as S;

    match (order.status, event) {
        (S::Submitted | S::Accepted, OrderEvent::Accepted { venue_order_id }) => Ok(Order {
            status: S::Accepted,
            venue_order_id: Some(known_or(order, venue_order_id)),
            ..order.clone()
        }),
        // A late acknowledgement: venues may send the ack on a different
        // channel than fills and cancels, so it can arrive after either.
        // Keep the status, learn the venue's id.
        (
            S::PartiallyFilled | S::PendingCancel | S::Filled | S::Cancelled,
            OrderEvent::Accepted { venue_order_id },
        ) => Ok(Order {
            venue_order_id: Some(known_or(order, venue_order_id)),
            ..order.clone()
        }),

        // Once anything has traded, the order cannot be "rejected" as a
        // whole; that would erase fills the account already reflects.
        (S::Submitted | S::Accepted | S::PendingCancel, OrderEvent::Rejected { reason })
            if order.filled_quantity.is_zero() =>
        {
            Ok(Order {
                status: S::Rejected,
                reason: Some(reason.clone()),
                ..order.clone()
            })
        }

        // A fill is applied whenever it arrives, even after a cancel was
        // confirmed: the venue traded, so the money moved.
        (
            S::Submitted | S::Accepted | S::PartiallyFilled | S::PendingCancel | S::Cancelled,
            OrderEvent::Filled {
                quantity, price, ..
            },
        ) => fill(order, *quantity, *price),

        (
            S::Submitted | S::Accepted | S::PartiallyFilled | S::PendingCancel,
            OrderEvent::Cancelled,
        )
        | (S::Cancelled, OrderEvent::Cancelled) => Ok(Order {
            status: S::Cancelled,
            ..order.clone()
        }),

        (S::PendingCancel, OrderEvent::CancelRejected { reason }) => Ok(Order {
            status: resting_status(order),
            reason: Some(reason.clone()),
            ..order.clone()
        }),
        // The usual race: the cancel crossed a final fill on the wire. The
        // order already reached a final state; the refusal changes nothing.
        (_, OrderEvent::CancelRejected { reason }) => Ok(Order {
            reason: Some(reason.clone()),
            ..order.clone()
        }),

        (from, event) => Err(TransitionError::Invalid {
            from,
            event: event_name(event).to_string(),
        }),
    }
}

fn fill(order: &Order, quantity: Decimal, price: Decimal) -> Result<Order, TransitionError> {
    if quantity <= Decimal::ZERO || price <= Decimal::ZERO {
        return Err(TransitionError::MalformedFill);
    }
    let remaining = order.remaining();
    if quantity > remaining {
        return Err(TransitionError::Overfill {
            remaining,
            fill: quantity,
        });
    }

    // Checked: a panic here would poison the engine's lock and take the
    // terminal down over one malformed report.
    let filled_quantity = order
        .filled_quantity
        .checked_add(quantity)
        .ok_or(TransitionError::OutOfRange)?;
    let avg_fill_price = order
        .avg_fill_price
        .unwrap_or_default()
        .checked_mul(order.filled_quantity)
        .and_then(|previous| price.checked_mul(quantity)?.checked_add(previous))
        .and_then(|notional| notional.checked_div(filled_quantity))
        .ok_or(TransitionError::OutOfRange)?;
    let status = if filled_quantity == order.quantity {
        OrderStatus::Filled
    } else if matches!(
        order.status,
        OrderStatus::PendingCancel | OrderStatus::Cancelled
    ) {
        order.status
    } else {
        OrderStatus::PartiallyFilled
    };

    Ok(Order {
        status,
        filled_quantity,
        avg_fill_price: Some(avg_fill_price),
        ..order.clone()
    })
}

/// The first venue id an order learns is the one it keeps.
fn known_or(order: &Order, venue_order_id: &model::VenueOrderId) -> model::VenueOrderId {
    order
        .venue_order_id
        .clone()
        .unwrap_or_else(|| venue_order_id.clone())
}

/// Where an order goes back to when a cancel attempt fails.
fn resting_status(order: &Order) -> OrderStatus {
    if order.filled_quantity > Decimal::ZERO {
        OrderStatus::PartiallyFilled
    } else if order.venue_order_id.is_some() {
        OrderStatus::Accepted
    } else {
        OrderStatus::Submitted
    }
}

fn event_name(event: &OrderEvent) -> &'static str {
    match event {
        OrderEvent::Accepted { .. } => "Accepted",
        OrderEvent::Rejected { .. } => "Rejected",
        OrderEvent::Filled { .. } => "Filled",
        OrderEvent::Cancelled => "Cancelled",
        OrderEvent::CancelRejected { .. } => "CancelRejected",
        _ => "an unrecognised event",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use model::{ClientOrderId, FillId, InstrumentId, Side, VenueOrderId};

    fn d(raw: &str) -> Decimal {
        raw.parse().expect("test decimal")
    }

    fn order(status: OrderStatus) -> Order {
        Order {
            id: ClientOrderId(1),
            venue_order_id: None,
            instrument: InstrumentId::new("sim", "AAA"),
            side: Side::Buy,
            quantity: d("10"),
            limit_price: Some(d("5")),
            status,
            filled_quantity: Decimal::ZERO,
            avg_fill_price: None,
            reason: None,
        }
    }

    fn accepted() -> OrderEvent {
        OrderEvent::Accepted {
            venue_order_id: VenueOrderId("V1".into()),
        }
    }

    fn filled(quantity: &str, price: &str) -> OrderEvent {
        OrderEvent::Filled {
            fill_id: FillId("F".into()),
            quantity: d(quantity),
            price: d(price),
        }
    }

    fn cancel_rejected() -> OrderEvent {
        OrderEvent::CancelRejected {
            reason: "too late".into(),
        }
    }

    #[test]
    fn acceptance_records_the_venue_order_id() {
        let next = apply(&order(OrderStatus::Submitted), &accepted()).unwrap();

        assert_eq!(next.status, OrderStatus::Accepted);
        assert_eq!(next.venue_order_id, Some(VenueOrderId("V1".into())));
    }

    #[test]
    fn rejection_is_terminal_and_keeps_the_reason() {
        let rejected = apply(
            &order(OrderStatus::Submitted),
            &OrderEvent::Rejected {
                reason: "market closed".into(),
            },
        )
        .unwrap();

        assert_eq!(rejected.status, OrderStatus::Rejected);
        assert_eq!(rejected.reason.as_deref(), Some("market closed"));
        assert!(apply(&rejected, &accepted()).is_err());
    }

    #[test]
    fn partial_fills_accumulate_to_a_volume_weighted_average() {
        let first = apply(&order(OrderStatus::Accepted), &filled("4", "10")).unwrap();
        let second = apply(&first, &filled("6", "20")).unwrap();

        assert_eq!(first.status, OrderStatus::PartiallyFilled);
        assert_eq!(second.status, OrderStatus::Filled);
        assert_eq!(second.filled_quantity, d("10"));
        assert_eq!(second.avg_fill_price, Some(d("16")));
    }

    #[test]
    fn a_fill_may_arrive_before_the_acceptance() {
        let next = apply(&order(OrderStatus::Submitted), &filled("10", "5")).unwrap();

        assert_eq!(next.status, OrderStatus::Filled);
    }

    #[test]
    fn a_late_acceptance_does_not_undo_a_partial_fill() {
        let partial = apply(&order(OrderStatus::Submitted), &filled("3", "5")).unwrap();

        let next = apply(&partial, &accepted()).unwrap();

        assert_eq!(next.status, OrderStatus::PartiallyFilled);
        assert!(next.venue_order_id.is_some());
    }

    #[test]
    fn overfill_is_refused_and_leaves_the_order_unchanged() {
        let before = order(OrderStatus::Accepted);

        let result = apply(&before, &filled("11", "5"));

        assert_eq!(
            result,
            Err(TransitionError::Overfill {
                remaining: d("10"),
                fill: d("11")
            })
        );
        assert_eq!(before.filled_quantity, Decimal::ZERO);
    }

    #[test]
    fn non_positive_fills_are_malformed() {
        let accepted = order(OrderStatus::Accepted);

        assert_eq!(
            apply(&accepted, &filled("0", "5")),
            Err(TransitionError::MalformedFill)
        );
        assert_eq!(
            apply(&accepted, &filled("1", "0")),
            Err(TransitionError::MalformedFill)
        );
    }

    #[test]
    fn refused_cancel_returns_the_order_to_where_it_was_resting() {
        let mut pending = order(OrderStatus::PendingCancel);
        pending.venue_order_id = Some(VenueOrderId("V1".into()));
        let unacked = order(OrderStatus::PendingCancel);
        let partial = Order {
            filled_quantity: d("2"),
            ..pending.clone()
        };

        assert_eq!(
            apply(&pending, &cancel_rejected()).unwrap().status,
            OrderStatus::Accepted
        );
        assert_eq!(
            apply(&unacked, &cancel_rejected()).unwrap().status,
            OrderStatus::Submitted
        );
        assert_eq!(
            apply(&partial, &cancel_rejected()).unwrap().status,
            OrderStatus::PartiallyFilled
        );
    }

    #[test]
    fn a_partial_fill_while_a_cancel_is_pending_stays_pending() {
        let next = apply(&order(OrderStatus::PendingCancel), &filled("4", "5")).unwrap();

        assert_eq!(next.status, OrderStatus::PendingCancel);
        assert_eq!(next.filled_quantity, d("4"));
    }

    #[test]
    fn cancel_rejected_after_the_final_fill_changes_nothing() {
        let done = apply(&order(OrderStatus::PendingCancel), &filled("10", "5")).unwrap();

        let next = apply(&done, &cancel_rejected()).unwrap();

        assert_eq!(next.status, OrderStatus::Filled);
    }

    #[test]
    fn a_fill_that_raced_the_cancel_is_still_applied() {
        let cancelled = order(OrderStatus::Cancelled);

        let next = apply(&cancelled, &filled("4", "5")).unwrap();

        assert_eq!(next.status, OrderStatus::Cancelled);
        assert_eq!(next.filled_quantity, d("4"));
    }

    #[test]
    fn terminal_orders_never_reopen() {
        for status in [OrderStatus::Rejected, OrderStatus::Denied] {
            assert!(apply(&order(status), &accepted()).is_err(), "{status:?}");
        }
        for status in [
            OrderStatus::Filled,
            OrderStatus::Rejected,
            OrderStatus::Denied,
        ] {
            assert!(
                apply(&order(status), &OrderEvent::Cancelled).is_err(),
                "{status:?}"
            );
        }
        for status in [OrderStatus::Filled, OrderStatus::Cancelled] {
            let late_ack = apply(&order(status), &accepted()).unwrap();
            assert_eq!(late_ack.status, status);
        }
    }

    #[test]
    fn a_late_acceptance_after_the_final_fill_records_the_venue_id() {
        let done = apply(&order(OrderStatus::Submitted), &filled("10", "5")).unwrap();

        let next = apply(&done, &accepted()).unwrap();

        assert_eq!(next.status, OrderStatus::Filled);
        assert_eq!(next.venue_order_id, Some(VenueOrderId("V1".into())));
    }

    #[test]
    fn the_first_venue_id_learned_is_kept() {
        let acked = apply(&order(OrderStatus::Submitted), &accepted()).unwrap();

        let next = apply(
            &acked,
            &OrderEvent::Accepted {
                venue_order_id: VenueOrderId("V2".into()),
            },
        )
        .unwrap();

        assert_eq!(next.venue_order_id, Some(VenueOrderId("V1".into())));
    }

    #[test]
    fn an_order_with_fills_cannot_be_rejected_as_a_whole() {
        let partial = Order {
            filled_quantity: d("3"),
            ..order(OrderStatus::PendingCancel)
        };

        let result = apply(
            &partial,
            &OrderEvent::Rejected {
                reason: "late".into(),
            },
        );

        assert!(matches!(result, Err(TransitionError::Invalid { .. })));
    }

    #[test]
    fn fill_arithmetic_that_would_overflow_is_refused_not_panicked() {
        let huge = Order {
            quantity: Decimal::MAX,
            ..order(OrderStatus::Accepted)
        };

        let result = apply(
            &huge,
            &OrderEvent::Filled {
                fill_id: FillId("F".into()),
                quantity: d("2"),
                price: Decimal::MAX,
            },
        );

        assert_eq!(result, Err(TransitionError::OutOfRange));
    }

    #[test]
    fn denied_orders_accept_no_fills() {
        assert!(apply(&order(OrderStatus::Denied), &filled("1", "5")).is_err());
    }
}
