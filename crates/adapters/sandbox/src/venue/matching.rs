//! Orders against the simulated book: checks, immediate fills, resting
//! orders, and the account they move.
//!
//! What an order does depends on its type and on the phase its market is
//! in. In continuous trading the book shows `display_size` at the touch,
//! and that is all an order can take at once, except a market order for
//! the day, which sweeps. During an auction or a break, orders queue; an
//! auction's end matches them ([`super::session`]). After the close, a
//! PLO-style order fills at the closing price.

use bus::Event;
use model::{
    AmendOrder, Decimal, FillId, InstrumentId, OrderEvent, OrderKind, Position, Quote,
    SessionPhase, Side, SubmitOrder, TimeInForce, UnixNanos, VenueOrderId,
};

use super::listing::{Listing, TOP_OF_BOOK_SIZE};
use super::{account_of, report, unsettled_in, Resting, Unsettled, VenueState};

const AVG_PRICE_DP: u32 = 8;

/// Why the venue would refuse this order under the listing's rules, if it would.
fn rule_breach(listing: &Listing, order: &SubmitOrder) -> Option<String> {
    let info = &listing.info;
    if !info.takes_quantity(order.quantity, order.order_type) {
        return Some(
            if info.is_odd_lot(order.quantity) && !info.odd_lot_types.is_empty() {
                "an odd lot must be one of the market's odd-lot order types".into()
            } else {
                format!(
                    "quantity must be a multiple of the lot size, {}",
                    info.lot_size
                )
            },
        );
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

/// Why the market would refuse this order type now, if it would. A market
/// with no schedule trades continuously and has no auctions.
fn type_refusal(
    state: &VenueState,
    market: &str,
    order: &SubmitOrder,
    now: UnixNanos,
) -> Option<String> {
    let Some(phase) = state.day.as_ref().and_then(|day| day.phase(market, now)) else {
        let session_only = matches!(
            order.order_type.time_in_force,
            TimeInForce::AtOpen | TimeInForce::AtClose | TimeInForce::AfterClose
        );
        return session_only.then(|| format!("{market} has no auctions or after-close session"));
    };
    if phase
        .order_types
        .iter()
        .any(|offered| offered.order_type == order.order_type)
    {
        return None;
    }
    let taken: Vec<&str> = phase.order_types.iter().map(|t| t.name.as_str()).collect();
    Some(if taken.is_empty() {
        format!("{market} takes no orders now")
    } else {
        format!("{market} takes only {} now", taken.join(", "))
    })
}

/// The phase `market` trades in at `now`: continuous for a market with no
/// schedule.
pub(super) fn phase_of(state: &VenueState, market: &str, now: UnixNanos) -> SessionPhase {
    state
        .day
        .as_ref()
        .and_then(|day| day.phase(market, now))
        .map_or(SessionPhase::Continuous, |phase| phase.phase)
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
    let market = book.listing.info.exchange.clone();
    if let Some(reason) = type_refusal(state, &market, &order, now) {
        return reject(reason);
    }
    if order.side == Side::Sell && !book.listing.info.short_selling {
        let sellable = sellable(state, &order.instrument);
        if order.quantity > sellable {
            return reject(format!(
                "not enough settled shares: {sellable} can be sold now"
            ));
        }
    }
    let quote = book.quote(&order.instrument, now);
    let display_size = book.listing.display_size;
    let touch = touch_price(&quote, order.side);
    let close = state
        .closes
        .get(&order.instrument)
        .copied()
        .unwrap_or(book.mid);
    let phase = phase_of(state, &market, now);
    let after_close = phase == SessionPhase::AfterClose;

    // Every later fill of this order trades at or inside `worst_price`, so
    // bounding its value here keeps the plain arithmetic in `fill` safe.
    let worst_price = match (after_close, order.limit_price) {
        (true, _) => close,
        (false, Some(limit)) => limit,
        (false, None) => touch,
    };
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
    match phase {
        SessionPhase::Continuous => {
            events.extend(trade_now(state, order, touch, display_size, now));
        }
        // At the closing price, if the order's limit allows it; otherwise it
        // waits, and the close cancels it.
        SessionPhase::AfterClose => {
            if order
                .limit_price
                .is_none_or(|limit| crosses(order.side, limit, close))
            {
                let quantity = order.quantity;
                events.extend(fill(state, &order, quantity, close, now));
            } else {
                state.resting.push(Resting {
                    limit: order.limit_price,
                    reserve: worst_price,
                    remaining: order.quantity,
                    order,
                });
            }
        }
        // Auctions and breaks collect orders; nothing trades until they end.
        _ => state.resting.push(Resting {
            limit: order.limit_price,
            reserve: worst_price,
            remaining: order.quantity,
            order,
        }),
    }
    events
}

/// Changes a resting order to new terms, as an exchange does: the listing's
/// rules apply to them, a buy's cash must cover them, a sell may not exceed
/// the settled shares where short selling is forbidden, and what has filled
/// stays filled. The order keeps resting; the walk fills it if its new price
/// is reached.
pub(super) fn amend_locked(
    state: &mut VenueState,
    amend: &AmendOrder,
    now: UnixNanos,
) -> Vec<Event> {
    let refuse = |reason: String| {
        vec![report(
            amend.order_id,
            OrderEvent::AmendRejected { reason },
            now,
        )]
    };
    let Some(index) = state
        .resting
        .iter()
        .position(|r| r.order.order_id == amend.order_id)
    else {
        return refuse("order is not open on the venue".into());
    };
    let (filled, own_remaining, own_reserve) = {
        let r = &state.resting[index];
        (r.order.quantity - r.remaining, r.remaining, r.reserve)
    };
    if amend.quantity <= filled {
        return refuse(format!(
            "the new quantity must be above the {filled} already filled"
        ));
    }
    let resting_order = &state.resting[index].order;
    let (Some(limit), OrderKind::Limit) = (amend.limit_price, resting_order.order_type.kind) else {
        return refuse("only a limit order can be amended".into());
    };
    let mut amended = resting_order.clone();
    amended.quantity = amend.quantity;
    amended.limit_price = Some(limit);
    let Some(book) = state.books.get(&amended.instrument) else {
        return refuse(format!(
            "{} is not listed on this venue",
            amended.instrument
        ));
    };
    if let Some(reason) = rule_breach(&book.listing, &amended) {
        return refuse(reason);
    }
    let market = book.listing.info.exchange.clone();
    if let Some(reason) = type_refusal(state, &market, &amended, now) {
        return refuse(reason);
    }
    let remaining = amend.quantity - filled;
    // As at submit: bounding the order's value keeps the plain arithmetic in
    // `fill` safe, whichever side it is.
    let in_range = remaining.checked_mul(limit).is_some_and(|n| {
        state.cash.checked_add(n).is_some() && state.cash.checked_sub(n).is_some()
    });
    if !in_range {
        return refuse("order value is out of range".into());
    }
    match amended.side {
        Side::Buy => {
            let available = available_cash(state)
                .zip(own_remaining.checked_mul(own_reserve))
                .and_then(|(free, own)| free.checked_add(own));
            let needed = remaining.checked_mul(limit);
            if !matches!((needed, available), (Some(n), Some(a)) if n <= a) {
                return refuse("insufficient cash for the new terms".into());
            }
        }
        Side::Sell if !book.listing.info.short_selling => {
            let sellable = sellable(state, &amended.instrument) + own_remaining;
            if remaining > sellable {
                return refuse(format!(
                    "not enough settled shares: {sellable} can be sold now"
                ));
            }
        }
        Side::Sell => {}
    }
    let resting = &mut state.resting[index];
    resting.order = amended;
    resting.limit = Some(limit);
    resting.reserve = limit;
    resting.remaining = remaining;
    vec![report(
        amend.order_id,
        OrderEvent::Amended {
            quantity: amend.quantity,
            limit_price: Some(limit),
        },
        now,
    )]
}

/// An order arriving in continuous trading: it takes what its type allows
/// of what is shown at the touch, then rests or is cancelled.
fn trade_now(
    state: &mut VenueState,
    order: SubmitOrder,
    touch: Decimal,
    shown: Decimal,
    now: UnixNanos,
) -> Vec<Event> {
    let marketable = order
        .limit_price
        .is_none_or(|limit| crosses(order.side, limit, touch));
    let quantity = order.quantity;
    let taken = match (order.order_type.kind, order.order_type.time_in_force) {
        _ if !marketable => Decimal::ZERO,
        (_, TimeInForce::FillOrKill) if quantity > shown => Decimal::ZERO,
        (_, TimeInForce::FillOrKill) => quantity,
        (OrderKind::Market, TimeInForce::Day) => quantity,
        _ => quantity.min(shown),
    };
    let mut events = Vec::new();
    if taken > Decimal::ZERO {
        events.extend(fill(state, &order, taken, touch, now));
    }
    let remaining = quantity - taken;
    if remaining <= Decimal::ZERO {
        return events;
    }
    // The rest of a market-to-limit order rests at the price it traded at.
    let resting_limit = match order.order_type.kind {
        OrderKind::MarketToLimit => Some(touch),
        _ => order.limit_price,
    };
    match (order.order_type.time_in_force, resting_limit) {
        (TimeInForce::Day, Some(limit)) => state.resting.push(Resting {
            limit: Some(limit),
            reserve: limit,
            remaining,
            order,
        }),
        _ => events.push(report(order.order_id, OrderEvent::Cancelled, now)),
    }
    events
}

/// Settled shares of `instrument` not already in a resting sell order.
fn sellable(state: &VenueState, instrument: &InstrumentId) -> Decimal {
    let held = state
        .positions
        .get(instrument)
        .map_or(Decimal::ZERO, |p| p.quantity);
    let resting: Decimal = state
        .resting
        .iter()
        .filter(|r| &r.order.instrument == instrument && r.order.side == Side::Sell)
        .map(|r| r.remaining)
        .sum();
    (held - unsettled_in(state, instrument) - resting).max(Decimal::ZERO)
}

/// The price a taker on `side` trades at.
fn touch_price(quote: &Quote, side: Side) -> Decimal {
    match side {
        Side::Buy => quote.ask,
        Side::Sell => quote.bid,
    }
}

pub(super) fn crosses(side: Side, limit: Decimal, touch: Decimal) -> bool {
    match side {
        Side::Buy => limit >= touch,
        Side::Sell => limit <= touch,
    }
}

/// Cash not already promised to resting buy orders at their reserved price.
/// `None` only if the reservations themselves overflow.
fn available_cash(state: &VenueState) -> Option<Decimal> {
    state
        .resting
        .iter()
        .filter(|r| r.order.side == Side::Buy)
        .try_fold(state.cash, |available, r| {
            let reserved = r.remaining.checked_mul(r.reserve)?;
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
        let Some(limit) = resting.limit else {
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
pub(super) fn fill(
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
    if order.side == Side::Buy {
        if let Some(day) = &state.day {
            let settles_on = day.trading_day(now) + day.settlement_days;
            state.unsettled.push(Unsettled {
                instrument: order.instrument.clone(),
                quantity,
                settles_on,
            });
        }
    }

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
    Some(Position::new(instrument.clone(), next, avg_price))
}
