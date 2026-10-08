//! The venue's record of every order it was sent this session, folded from
//! the reports it publishes. It is what a broker's API returns when an
//! adapter reads the session's orders back after a reconnect (ADR 0012):
//! the adapter publishes it as snapshots, and core catches up on what the
//! dropped stream did not deliver.

use std::collections::BTreeMap;

use bus::Event;
use model::{
    ClientOrderId, Decimal, OrderEvent, SubmitOrder, VenueOrder, VenueOrderId, VenueOrderState,
};

struct Record {
    venue_order_id: Option<VenueOrderId>,
    quantity: Decimal,
    limit_price: Option<Decimal>,
    filled: Decimal,
    /// Quantity times price over every fill, for the average.
    notional: Decimal,
    state: VenueOrderState,
}

#[derive(Default)]
pub(super) struct Ledger {
    orders: BTreeMap<ClientOrderId, Record>,
}

impl Ledger {
    /// Starts the record of an order the venue was just sent.
    pub(super) fn open(&mut self, order: &SubmitOrder) {
        self.orders.insert(
            order.order_id,
            Record {
                venue_order_id: None,
                quantity: order.quantity,
                limit_price: order.limit_price,
                filled: Decimal::ZERO,
                notional: Decimal::ZERO,
                state: VenueOrderState::Working,
            },
        );
    }

    /// Folds in what the venue just reported about an order.
    pub(super) fn note(&mut self, event: &Event) {
        let Event::Execution(report) = event else {
            return;
        };
        let Some(record) = self.orders.get_mut(&report.order_id) else {
            return;
        };
        match &report.event {
            OrderEvent::Accepted { venue_order_id } => {
                record.venue_order_id = Some(venue_order_id.clone());
            }
            OrderEvent::Rejected { reason } => {
                record.state = VenueOrderState::Rejected {
                    reason: reason.clone(),
                };
            }
            OrderEvent::Filled {
                quantity, price, ..
            } => {
                record.filled += *quantity;
                record.notional = quantity
                    .checked_mul(*price)
                    .and_then(|value| record.notional.checked_add(value))
                    .unwrap_or(record.notional);
                if record.filled >= record.quantity {
                    record.state = VenueOrderState::Filled;
                }
            }
            OrderEvent::Cancelled if record.state == VenueOrderState::Working => {
                record.state = VenueOrderState::Cancelled;
            }
            OrderEvent::Amended {
                quantity,
                limit_price,
            } => {
                record.quantity = *quantity;
                record.limit_price = *limit_price;
            }
            _ => {}
        }
    }

    /// Every order of the session as the venue holds it now.
    pub(super) fn views(&self) -> Vec<(ClientOrderId, VenueOrder)> {
        self.orders
            .iter()
            .map(|(id, record)| {
                let average = (!record.filled.is_zero())
                    .then(|| record.notional.checked_div(record.filled))
                    .flatten();
                let view = VenueOrder::new(record.state.clone(), record.filled, average)
                    .with_terms(record.quantity, record.limit_price);
                let view = match &record.venue_order_id {
                    Some(venue_id) => view.with_venue_order_id(venue_id.clone()),
                    None => view,
                };
                (*id, view)
            })
            .collect()
    }
}
