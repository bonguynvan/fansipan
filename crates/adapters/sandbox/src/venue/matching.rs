//! Orders against the simulated book: checks, immediate fills, resting
//! orders, and the account they move.

use bus::Event;
use model::{
    Decimal, FillId, InstrumentId, OrderEvent, Position, Quote, Side, SubmitOrder, UnixNanos,
    VenueOrderId,
};

use super::listing::{Listing, TOP_OF_BOOK_SIZE};
use super::{account_of, report, Resting, VenueState};

const AVG_PRICE_DP: u32 = 8;

/// Why the venue would refuse this order under the listing's rules, if it would.
fn rule_breach(listing: &Listing, order: &SubmitOrder) -> Option<String> {
    if !listing.info.is_lot_multiple(order.quantity) {
        return Some(format!(
            "quantity must be a multiple of the lot size, {}",
            listing.info.lot_size
        ));
    }
    let price = order.limit_price?;
    if !listing.ticks().is_on_tick(price) {
        return Some(format!(
            "price must be a multiple of the tick size, {}",
            listing.ticks().tick_at(price)
        ));
    }
    if !listing.band.contains(price) {
        return Some("price is outside today's ceiling and floor".into());
    }
    None
}

pub(super) fn submit_locked(
    state: &mut VenueState,
    order: SubmitOrder,
    now: UnixNanos,
) -> Vec<Event> {
    let reject =
        |reason: String| vec![report(order.order_id, OrderEvent::Rejected { reason }, now)];

    let Some(book) = state.books.get(&order.instrument) else {
        return reject(format!("{} is not listed on this venue", order.instrument));
    };
    if order.quantity <= Decimal::ZERO {
        return reject("quantity must be positive".into());
    }
    if let Some(reason) = rule_breach(&book.listing, &order) {
        return reject(reason);
    }
    let quote = book.quote(&order.instrument, now);
    let display_size = book.listing.display_size;
    let touch = touch_price(&quote, order.side);

    // Every later fill of this order trades at or inside `worst_price`, so
    // bounding its value here keeps the plain arithmetic in `fill` safe.
    let worst_price = order.limit_price.unwrap_or(touch);
    let notional = order
        .quantity
        .checked_mul(worst_price)
        .filter(|n| state.cash.checked_add(*n).is_some() && state.cash.checked_sub(*n).is_some());
    let Some(notional) = notional else {
        return reject("order value is out of range".into());
    };
    if order.side == Side::Buy {
        let available = available_cash(state);
        if available.is_none_or(|available| notional > available) {
            return reject(format!(
                "insufficient cash: needs {notional}, {} available",
                available.unwrap_or_default()
            ));
        }
    }

    let venue_order_id = VenueOrderId(format!("SIM-{}", state.next_venue_id));
    state.next_venue_id += 1;
    let mut events = vec![report(
        order.order_id,
        OrderEvent::Accepted { venue_order_id },
        now,
    )];

    let marketable = order
        .limit_price
        .is_none_or(|limit| crosses(order.side, limit, touch));
    let mut remaining = order.quantity;
    if marketable {
        // A market order sweeps; a marketable limit takes what is shown.
        let quantity = match order.limit_price {
            None => remaining,
            Some(_) => remaining.min(display_size),
        };
        events.extend(fill(state, &order, quantity, touch, now));
        remaining -= quantity;
    }
    if remaining > Decimal::ZERO {
        state.resting.push(Resting { order, remaining });
    }
    events
}

/// The price a taker on `side` trades at.
fn touch_price(quote: &Quote, side: Side) -> Decimal {
    match side {
        Side::Buy => quote.ask,
        Side::Sell => quote.bid,
    }
}

fn crosses(side: Side, limit: Decimal, touch: Decimal) -> bool {
    match side {
        Side::Buy => limit >= touch,
        Side::Sell => limit <= touch,
    }
}

/// Cash not already promised to resting buy orders at their limit price.
/// `None` only if the reservations themselves overflow.
fn available_cash(state: &VenueState) -> Option<Decimal> {
    state
        .resting
        .iter()
        .filter(|r| r.order.side == Side::Buy)
        .try_fold(state.cash, |available, r| {
            let reserved = r
                .remaining
                .checked_mul(r.order.limit_price.unwrap_or_default())?;
            available.checked_sub(reserved)
        })
}

/// Fills resting orders the new quote reaches, at most the displayed size
/// each. A resting order is the maker, so it trades at its own limit price,
/// not at the touch.
pub(super) fn match_resting(state: &mut VenueState, quote: &Quote, now: UnixNanos) -> Vec<Event> {
    let display_size = state
        .books
        .get(&quote.instrument)
        .map_or(TOP_OF_BOOK_SIZE, |book| book.listing.display_size);
    let mut fills = Vec::new();
    for index in 0..state.resting.len() {
        let resting = &state.resting[index];
        if resting.order.instrument != quote.instrument {
            continue;
        }
        let Some(limit) = resting.order.limit_price else {
            continue;
        };
        if !crosses(
            resting.order.side,
            limit,
            touch_price(quote, resting.order.side),
        ) {
            continue;
        }
        let quantity = resting.remaining.min(display_size);
        fills.push((index, quantity, limit));
    }

    let mut events = Vec::new();
    for (index, quantity, price) in &fills {
        let order = state.resting[*index].order.clone();
        state.resting[*index].remaining -= *quantity;
        events.extend(fill(state, &order, *quantity, *price, now));
    }
    state.resting.retain(|r| r.remaining > Decimal::ZERO);
    events
}

/// Books one fill and returns the execution report plus the account update.
fn fill(
    state: &mut VenueState,
    order: &SubmitOrder,
    quantity: Decimal,
    price: Decimal,
    now: UnixNanos,
) -> Vec<Event> {
    let signed = match order.side {
        Side::Buy => quantity,
        Side::Sell => -quantity,
    };
    state.cash -= signed * price;
    let next = net_position(
        state.positions.get(&order.instrument),
        &order.instrument,
        signed,
        price,
    );
    match next {
        Some(position) => state.positions.insert(order.instrument.clone(), position),
        None => state.positions.remove(&order.instrument),
    };

    let fill_id = FillId(format!("SIM-F{}", state.next_fill_id));
    state.next_fill_id += 1;
    vec![
        report(
            order.order_id,
            OrderEvent::Filled {
                fill_id,
                quantity,
                price,
            },
            now,
        ),
        Event::AccountUpdate(account_of(state)),
    ]
}

/// The position after a signed fill, or `None` once it is flat. Adding to a
/// position blends the average price; reducing keeps it; flipping through
/// zero starts the new side at the fill price.
fn net_position(
    current: Option<&Position>,
    instrument: &InstrumentId,
    signed: Decimal,
    price: Decimal,
) -> Option<Position> {
    let (quantity, avg) = current.map_or((Decimal::ZERO, Decimal::ZERO), |p| {
        (p.quantity, p.avg_price)
    });
    let next = quantity + signed;
    if next.is_zero() {
        return None;
    }
    let adding = quantity.is_zero() || quantity.is_sign_positive() == signed.is_sign_positive();
    let flipped = !adding && next.is_sign_positive() != quantity.is_sign_positive();
    let avg_price = if adding {
        ((quantity * avg + signed * price) / next).round_dp(AVG_PRICE_DP)
    } else if flipped {
        price
    } else {
        avg
    };
    Some(Position {
        instrument: instrument.clone(),
        quantity: next,
        avg_price,
    })
}
