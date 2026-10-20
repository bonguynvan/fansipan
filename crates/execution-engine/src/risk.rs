//! Pre-trade checks. Runs before any order reaches an ExecutionClient.
//!
//! First the trader's own switch ([`check_trading_state`]: halted, or
//! taking only orders that reduce a position). Then two families: this
//! terminal's own limits ([`check`]), and the market's
//! rules as the adapter published them: the instrument's status
//! ([`check_instrument_status`]: halted, suspended, short sales
//! restricted), its rules
//! ([`check_market_rules`]: lot size, tick size, daily price band), for the
//! moment ([`check_session`]: the order types its market takes in the
//! current phase of the day), and for the holding ([`check_sellable`]: no
//! selling beyond the settled shares where short selling is forbidden). An
//! order a venue would refuse for breaking its rules is refused here first,
//! with a reason.
//!
//! An order that passes every check comes out [`Cleared`], and only this
//! module makes one; the adapter's client takes nothing else
//! ([`crate::gate::Gate`]).

use std::fmt;

use model::{
    AmendOrder, ClientOrderId, Decimal, InstrumentInfo, InstrumentStatus, NewOrder, OrderKind,
    OrderType, Position, PriceBand, SessionPhase, SessionStatus, Side, SubmitOrder, TimeInForce,
    TradingStatus, VenueOrderId,
};

mod status;

pub use status::check_instrument_status;

/// Terms that passed every check risk runs. Nothing outside this module
/// can make one, so holding one is proof that risk ran. It is not `Clone`:
/// one clearance is good for one send, since a sell cleared against the
/// holding must not go twice.
#[derive(Debug, PartialEq)]
pub struct Cleared<T>(T);

impl<T> Cleared<T> {
    pub(crate) fn into_inner(self) -> T {
        self.0
    }
}

impl Cleared<NewOrder> {
    /// The cleared order, to be sent as `id`.
    pub fn submit(self, id: ClientOrderId) -> Cleared<SubmitOrder> {
        Cleared(SubmitOrder::new(id, self.0))
    }

    /// The cleared terms, as the new terms of order `id`.
    pub fn amend(
        self,
        id: ClientOrderId,
        venue_order_id: Option<VenueOrderId>,
    ) -> Cleared<AmendOrder> {
        let terms = self.0;
        Cleared(AmendOrder::new(
            id,
            venue_order_id,
            terms.instrument,
            terms.quantity,
            terms.limit_price,
        ))
    }
}

/// Whether this terminal sends new orders: the trader's kill switch.
/// Cancels are never stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TradingState {
    /// Orders go out as the other checks allow.
    #[default]
    Active,
    /// Only orders that shrink a position: a sell no larger than the long
    /// position, a buy no larger than the short one, less what working
    /// orders on that side already take.
    Reducing,
    /// No new order and no amendment.
    Halted,
}

/// What the checks read besides the order: this terminal's switch and
/// limits, and the market's rules, session, and holding as the adapter and
/// the venue last reported them.
pub(crate) struct Context<'a> {
    pub trading: TradingState,
    /// False for an amendment that does not raise the order's quantity,
    /// which reducing lets through: it can only shrink what is working.
    pub adds: bool,
    pub limits: &'a RiskLimits,
    pub info: Option<&'a InstrumentInfo>,
    pub band: Option<&'a PriceBand>,
    pub session: Option<&'a SessionStatus>,
    /// The instrument's own status, if its venue published one.
    pub status: Option<&'a InstrumentStatus>,
    pub position: Option<&'a Position>,
    /// Shares the working sell orders take, other than the order being
    /// amended.
    pub working_sells: Decimal,
    /// The same for working buy orders.
    pub working_buys: Decimal,
    /// What has filled of the order being amended, which its new terms do
    /// not sell again; zero for a new order.
    pub filled: Decimal,
}

/// Every check, in order: the trading state, this terminal's limits, then
/// the instrument's status, the market's rules, session, and the holding a
/// sell may take.
pub(crate) fn clear(
    terms: NewOrder,
    context: &Context<'_>,
) -> Result<Cleared<NewOrder>, RiskReject> {
    let selling = NewOrder::new(
        terms.instrument.clone(),
        terms.side,
        terms.quantity - context.filled,
        terms.order_type,
        terms.limit_price,
    );
    let working = match terms.side {
        Side::Buy => context.working_buys,
        Side::Sell => context.working_sells,
    };
    let held = context.position.map_or(Decimal::ZERO, |p| p.quantity);
    if context.adds || context.trading == TradingState::Halted {
        check_trading_state(&selling, context.trading, held, working)?;
    }
    check(&terms, context.limits)?;
    check_instrument_status(
        &selling,
        context.status,
        context.position,
        context.working_sells,
    )?;
    check_market_rules(&terms, context.info, context.band)?;
    check_session(&terms, context.session)?;
    check_sellable(
        &selling,
        context.info,
        context.position,
        context.working_sells,
    )?;
    Ok(Cleared(terms))
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RiskLimits {
    pub max_order_quantity: Decimal,
    /// Cap on quantity × limit price. Market orders carry no price here and
    /// are bounded by quantity alone until risk can read a reference quote.
    pub max_order_notional: Decimal,
}

#[derive(Debug, Clone, PartialEq)]
pub enum RiskReject {
    QuantityNotPositive,
    QuantityTooLarge {
        max: Decimal,
    },
    /// A limit order with no limit price.
    LimitPriceMissing,
    /// A price on an order whose kind takes none, such as a market order.
    PriceOnUnpricedOrder,
    LimitPriceNotPositive,
    NotionalTooLarge {
        max: Decimal,
    },
    NotLotMultiple {
        lot: Decimal,
    },
    /// Neither whole lots nor an odd lot, where the market takes odd lots.
    NotLotsOrOddLot {
        lot: Decimal,
    },
    /// An odd lot of a type the market does not take odd lots as.
    OddLotType {
        lot: Decimal,
        allowed: Vec<String>,
    },
    PriceNotOnTick {
        tick: Decimal,
    },
    PriceAboveCeiling {
        ceiling: Decimal,
    },
    PriceBelowFloor {
        floor: Decimal,
    },
    /// The instrument's market does not take this order type in the current
    /// phase of its day. `accepted` are the venue's names for what it takes.
    TypeNotTaken {
        market: String,
        phase: String,
        accepted: Vec<String>,
    },
    /// The trader halted trading in this terminal.
    TradingHalted,
    /// The terminal takes only orders that reduce a position, and this one
    /// would not, or would trade more than `closable`.
    ReducingOnly {
        closable: Decimal,
    },
    /// A sell beyond what may be sold now, where short selling is
    /// forbidden: the settled shares less those already in working sells.
    NotEnoughToSell {
        sellable: Decimal,
        held: Decimal,
        unsettled: Decimal,
        working: Decimal,
    },
    /// The instrument does not trade now, and its venue takes no orders
    /// for it.
    NotTrading {
        status: TradingStatus,
        reason: Option<String>,
    },
    /// A short sale without a limit price, under a short-sale restriction:
    /// only `long` can be sold at market.
    ShortSaleRestricted {
        long: Decimal,
    },
}

impl fmt::Display for RiskReject {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::QuantityNotPositive => f.write_str("quantity must be greater than zero"),
            Self::QuantityTooLarge { max } => {
                write!(f, "quantity is above the per-order limit of {max}")
            }
            Self::LimitPriceMissing => f.write_str("a limit order needs a limit price"),
            Self::PriceOnUnpricedOrder => f.write_str("only a limit order takes a price"),
            Self::LimitPriceNotPositive => f.write_str("limit price must be greater than zero"),
            Self::NotionalTooLarge { max } => {
                write!(f, "order value is above the per-order limit of {max}")
            }
            Self::NotLotMultiple { lot } => {
                write!(f, "quantity must be a multiple of the lot size, {lot}")
            }
            Self::NotLotsOrOddLot { lot } => write!(
                f,
                "quantity must be whole lots of {lot}, or an odd lot below {lot}"
            ),
            Self::OddLotType { lot, allowed } => write!(
                f,
                "an odd lot (fewer than {lot}) can only be {}",
                allowed.join(" or ")
            ),
            Self::PriceNotOnTick { tick } => {
                write!(f, "limit price must be a multiple of the tick size, {tick}")
            }
            Self::PriceAboveCeiling { ceiling } => {
                write!(f, "limit price is above today's ceiling of {ceiling}")
            }
            Self::PriceBelowFloor { floor } => {
                write!(f, "limit price is below today's floor of {floor}")
            }
            Self::TypeNotTaken {
                market,
                phase,
                accepted,
            } if accepted.is_empty() => write!(f, "{market} is in {phase} and takes no orders now"),
            Self::TypeNotTaken {
                market,
                phase,
                accepted,
            } => write!(
                f,
                "{market} is in {phase} and takes only {} now",
                accepted.join(", ")
            ),
            Self::TradingHalted => f.write_str("trading is halted in this terminal"),
            Self::ReducingOnly { closable } if closable.is_zero() => f.write_str(
                "only orders that reduce a position are taken now, and this one would not",
            ),
            Self::ReducingOnly { closable } => write!(
                f,
                "only orders that reduce a position are taken now: at most {closable}"
            ),
            Self::NotEnoughToSell {
                sellable,
                held,
                unsettled,
                working,
            } => write!(
                f,
                "only {sellable} can be sold now: {held} held, {unsettled} not settled yet, \
                 {working} in working sell orders"
            ),
            Self::NotTrading { status, reason } => {
                let what = match status {
                    TradingStatus::Suspended => "suspended",
                    _ => "halted",
                };
                match reason {
                    Some(reason) => write!(
                        f,
                        "the instrument is {what} ({reason}) and takes no orders now"
                    ),
                    None => write!(f, "the instrument is {what} and takes no orders now"),
                }
            }
            Self::ShortSaleRestricted { long } => write!(
                f,
                "short sales are restricted: a sell beyond {long} needs a limit price"
            ),
        }
    }
}

/// The trader's switch against an order. `held` is the position in its
/// instrument, negative when short; `working` is what working orders on the
/// order's side already take.
pub fn check_trading_state(
    order: &NewOrder,
    state: TradingState,
    held: Decimal,
    working: Decimal,
) -> Result<(), RiskReject> {
    match state {
        TradingState::Active => Ok(()),
        TradingState::Halted => Err(RiskReject::TradingHalted),
        TradingState::Reducing => {
            let open = match order.side {
                Side::Sell => held,
                Side::Buy => -held,
            };
            // Saturating: sizes from outside cannot be trusted to subtract,
            // and nothing closable is the safe answer.
            let closable = open
                .checked_sub(working)
                .unwrap_or(Decimal::ZERO)
                .max(Decimal::ZERO);
            if order.quantity <= closable {
                Ok(())
            } else {
                Err(RiskReject::ReducingOnly { closable })
            }
        }
    }
}

pub fn check(order: &NewOrder, limits: &RiskLimits) -> Result<(), RiskReject> {
    if order.quantity <= Decimal::ZERO {
        return Err(RiskReject::QuantityNotPositive);
    }
    if order.quantity > limits.max_order_quantity {
        return Err(RiskReject::QuantityTooLarge {
            max: limits.max_order_quantity,
        });
    }
    match (order.order_type.kind.is_priced(), order.limit_price) {
        (true, None) => return Err(RiskReject::LimitPriceMissing),
        (false, Some(_)) => return Err(RiskReject::PriceOnUnpricedOrder),
        _ => {}
    }
    if let Some(price) = order.limit_price {
        if price <= Decimal::ZERO {
            return Err(RiskReject::LimitPriceNotPositive);
        }
        let within = order
            .quantity
            .checked_mul(price)
            .is_some_and(|notional| notional <= limits.max_order_notional);
        if !within {
            return Err(RiskReject::NotionalTooLarge {
                max: limits.max_order_notional,
            });
        }
    }
    Ok(())
}

/// The market's own rules for the instrument, where the adapter published
/// them. An instrument with no published rules passes: the absence of a
/// rule is not a reason to refuse an order.
pub fn check_market_rules(
    order: &NewOrder,
    info: Option<&InstrumentInfo>,
    band: Option<&PriceBand>,
) -> Result<(), RiskReject> {
    if let Some(info) = info {
        if !info.takes_quantity(order.quantity, order.order_type) {
            let lot = info.lot_size;
            return Err(if info.odd_lot_types.is_empty() {
                RiskReject::NotLotMultiple { lot }
            } else if info.is_odd_lot(order.quantity) {
                RiskReject::OddLotType {
                    lot,
                    allowed: info.odd_lot_types.iter().map(|t| type_words(*t)).collect(),
                }
            } else {
                RiskReject::NotLotsOrOddLot { lot }
            });
        }
    }
    let Some(price) = order.limit_price else {
        return Ok(());
    };
    if let Some(info) = info {
        if !info.ticks.is_on_tick(price) {
            return Err(RiskReject::PriceNotOnTick {
                tick: info.ticks.tick_at(price),
            });
        }
    }
    if let Some(band) = band {
        if let Some(ceiling) = band.ceiling.filter(|c| price > *c) {
            return Err(RiskReject::PriceAboveCeiling { ceiling });
        }
        if let Some(floor) = band.floor.filter(|f| price < *f) {
            return Err(RiskReject::PriceBelowFloor { floor });
        }
    }
    Ok(())
}

/// The order type against what the instrument's market takes now, where
/// the adapter published the market's session. No session, no check: the
/// venue's own refusal is then the guard.
pub fn check_session(order: &NewOrder, session: Option<&SessionStatus>) -> Result<(), RiskReject> {
    let Some(session) = session else {
        return Ok(());
    };
    if session.accepts(order.order_type).is_some() {
        return Ok(());
    }
    Err(RiskReject::TypeNotTaken {
        market: session.market.clone(),
        phase: session
            .name
            .clone()
            .unwrap_or_else(|| phase_words(session.phase).to_string()),
        accepted: session.order_types.iter().map(|t| t.name.clone()).collect(),
    })
}

/// A sell against the holding, where the instrument's market forbids short
/// selling: at most the settled shares, less what this session's working
/// sell orders (`working`) already take. An instrument without published
/// rules, or one that allows short selling, is not checked; nor is a buy.
pub fn check_sellable(
    order: &NewOrder,
    info: Option<&InstrumentInfo>,
    position: Option<&Position>,
    working: Decimal,
) -> Result<(), RiskReject> {
    let forbids_short = info.is_some_and(|info| !info.short_selling);
    if order.side != Side::Sell || !forbids_short {
        return Ok(());
    }
    let held = position.map_or(Decimal::ZERO, |p| p.quantity.max(Decimal::ZERO));
    let settled = position.map_or(Decimal::ZERO, Position::sellable);
    let sellable = sellable_now(position, working);
    if order.quantity <= sellable {
        return Ok(());
    }
    Err(RiskReject::NotEnoughToSell {
        sellable,
        held,
        unsettled: held - settled,
        working,
    })
}

/// What a sell may take now where short selling is forbidden: the settled
/// shares, less what working sell orders (`working`) already take. The one
/// figure both [`check_sellable`] and the ticket's capacity use.
pub fn sellable_now(position: Option<&Position>, working: Decimal) -> Decimal {
    let settled = position.map_or(Decimal::ZERO, Position::sellable);
    (settled - working).max(Decimal::ZERO)
}

/// An order type in words, for a reason: "a limit order for the day".
fn type_words(order_type: OrderType) -> String {
    let kind = match order_type.kind {
        OrderKind::Limit => "a limit order",
        OrderKind::Market => "a market order",
        OrderKind::MarketToLimit => "a market-to-limit order",
        _ => "an order of another kind",
    };
    let time_in_force = match order_type.time_in_force {
        TimeInForce::Day => "for the day",
        TimeInForce::ImmediateOrCancel => "immediate or cancel",
        TimeInForce::FillOrKill => "fill or kill",
        TimeInForce::AtOpen => "at the open",
        TimeInForce::AtClose => "at the close",
        TimeInForce::AfterClose => "after the close",
        _ => "of another time in force",
    };
    format!("{kind} {time_in_force}")
}

fn phase_words(phase: SessionPhase) -> &'static str {
    match phase {
        SessionPhase::PreOpen => "pre-open",
        SessionPhase::OpeningAuction => "its opening auction",
        SessionPhase::Continuous => "continuous trading",
        SessionPhase::Break => "a break",
        SessionPhase::ClosingAuction => "its closing auction",
        SessionPhase::AfterClose => "its after-close session",
        SessionPhase::Closed => "a closed session",
        _ => "an unknown phase",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use model::{
        InstrumentId, NamedOrderType, OrderKind, OrderType, Side, TickTable, TimeInForce, UnixNanos,
    };

    const LIMITS: RiskLimits = RiskLimits {
        max_order_quantity: Decimal::ONE_HUNDRED,
        max_order_notional: Decimal::ONE_THOUSAND,
    };

    fn d(raw: &str) -> Decimal {
        raw.parse().expect("test decimal")
    }

    /// A limit order with a price, a market order without.
    fn order(quantity: &str, limit_price: Option<&str>) -> NewOrder {
        let instrument = InstrumentId::new("sim", "AAA");
        match limit_price {
            Some(price) => NewOrder::limit(instrument, Side::Buy, d(quantity), d(price)),
            None => NewOrder::market(instrument, Side::Buy, d(quantity)),
        }
    }

    #[test]
    fn accepts_an_order_within_limits() {
        assert_eq!(check(&order("10", Some("5")), &LIMITS), Ok(()));
    }

    #[test]
    fn accepts_an_order_exactly_at_the_cap() {
        assert_eq!(check(&order("100", None), &LIMITS), Ok(()));
    }

    #[test]
    fn rejects_zero_or_negative_quantity() {
        assert_eq!(
            check(&order("0", None), &LIMITS),
            Err(RiskReject::QuantityNotPositive)
        );
        assert_eq!(
            check(&order("-1", None), &LIMITS),
            Err(RiskReject::QuantityNotPositive)
        );
    }

    #[test]
    fn rejects_quantity_above_the_cap() {
        assert_eq!(
            check(&order("100.5", None), &LIMITS),
            Err(RiskReject::QuantityTooLarge { max: d("100") })
        );
    }

    #[test]
    fn rejects_non_positive_limit_price() {
        assert_eq!(
            check(&order("1", Some("0")), &LIMITS),
            Err(RiskReject::LimitPriceNotPositive)
        );
    }

    #[test]
    fn rejects_a_limit_order_worth_more_than_the_notional_cap() {
        assert_eq!(
            check(&order("100", Some("10.01")), &LIMITS),
            Err(RiskReject::NotionalTooLarge { max: d("1000") })
        );
        assert_eq!(check(&order("100", Some("10")), &LIMITS), Ok(()));
    }

    #[test]
    fn an_absurd_limit_price_is_denied_not_overflowed() {
        let absurd = order("2", Some("79228162514264337593543950335"));

        assert_eq!(
            check(&absurd, &LIMITS),
            Err(RiskReject::NotionalTooLarge { max: d("1000") })
        );
    }

    fn vnm() -> (InstrumentInfo, PriceBand) {
        let ticks = TickTable::new(vec![
            (d("0"), d("10")),
            (d("10000"), d("50")),
            (d("50000"), d("100")),
        ])
        .unwrap();
        let id = InstrumentId::new("sim", "AAA");
        let band =
            PriceBand::from_fraction(id.clone(), d("62500"), d("0.07"), &ticks, UnixNanos(0))
                .unwrap();
        (InstrumentInfo::new(id, d("100"), ticks), band)
    }

    #[test]
    fn market_rules_accept_a_valid_order() {
        let (info, band) = vnm();

        assert_eq!(
            check_market_rules(&order("300", Some("62600")), Some(&info), Some(&band)),
            Ok(())
        );
        assert_eq!(
            check_market_rules(&order("300", None), Some(&info), Some(&band)),
            Ok(())
        );
    }

    #[test]
    fn market_rules_refuse_an_odd_lot() {
        let (info, band) = vnm();

        assert_eq!(
            check_market_rules(&order("150", Some("62600")), Some(&info), Some(&band)),
            Err(RiskReject::NotLotMultiple { lot: d("100") })
        );
    }

    #[test]
    fn market_rules_refuse_a_price_between_ticks() {
        let (info, band) = vnm();

        assert_eq!(
            check_market_rules(&order("100", Some("62550")), Some(&info), Some(&band)),
            Err(RiskReject::PriceNotOnTick { tick: d("100") })
        );
    }

    #[test]
    fn market_rules_refuse_a_price_outside_the_band() {
        let (info, band) = vnm();

        assert_eq!(
            check_market_rules(&order("100", Some("66900")), Some(&info), Some(&band)),
            Err(RiskReject::PriceAboveCeiling {
                ceiling: d("66800")
            })
        );
        assert_eq!(
            check_market_rules(&order("100", Some("58100")), Some(&info), Some(&band)),
            Err(RiskReject::PriceBelowFloor { floor: d("58200") })
        );
    }

    #[test]
    fn a_limit_exactly_at_the_ceiling_or_floor_is_inside_the_band() {
        let (info, band) = vnm();

        assert_eq!(
            check_market_rules(&order("100", Some("66800")), Some(&info), Some(&band)),
            Ok(())
        );
        assert_eq!(
            check_market_rules(&order("100", Some("58200")), Some(&info), Some(&band)),
            Ok(())
        );
    }

    #[test]
    fn a_market_order_has_no_price_for_the_band_to_refuse() {
        let (info, band) = vnm();

        assert_eq!(
            check_market_rules(&order("100", None), Some(&info), Some(&band)),
            Ok(())
        );
    }

    #[test]
    fn a_sell_must_be_a_whole_number_of_lots_too() {
        let (info, band) = vnm();
        let sell = NewOrder::limit(
            InstrumentId::new("sim", "AAA"),
            Side::Sell,
            d("150"),
            d("62500"),
        );

        assert_eq!(
            check_market_rules(&sell, Some(&info), Some(&band)),
            Err(RiskReject::NotLotMultiple { lot: d("100") })
        );
    }

    #[test]
    fn a_lot_size_of_zero_or_less_means_no_lot_rule() {
        let (_, band) = vnm();
        let ticks = TickTable::flat(d("1")).unwrap();
        let unlotted = InstrumentInfo::new(InstrumentId::new("sim", "AAA"), d("0"), ticks);

        assert_eq!(
            check_market_rules(&order("7", Some("62500")), Some(&unlotted), Some(&band)),
            Ok(())
        );
    }

    #[test]
    fn an_instrument_without_published_rules_passes() {
        assert_eq!(
            check_market_rules(&order("7", Some("1.234")), None, None),
            Ok(())
        );
    }

    #[test]
    fn reasons_read_as_sentences() {
        let reason = RiskReject::QuantityTooLarge { max: d("1000") }.to_string();

        assert_eq!(reason, "quantity is above the per-order limit of 1000");
    }

    #[test]
    fn a_price_goes_with_a_limit_order_and_no_other() {
        let instrument = InstrumentId::new("sim", "AAA");
        let unpriced_limit = NewOrder::new(
            instrument.clone(),
            Side::Buy,
            d("1"),
            OrderType::LIMIT,
            None,
        );
        let priced_market = NewOrder::new(
            instrument,
            Side::Buy,
            d("1"),
            OrderType::MARKET,
            Some(d("5")),
        );

        assert_eq!(
            check(&unpriced_limit, &LIMITS),
            Err(RiskReject::LimitPriceMissing)
        );
        assert_eq!(
            check(&priced_market, &LIMITS),
            Err(RiskReject::PriceOnUnpricedOrder)
        );
    }

    fn ato_session() -> SessionStatus {
        let ato = OrderType::new(OrderKind::Market, TimeInForce::AtOpen);
        SessionStatus::new("HOSE", SessionPhase::OpeningAuction, UnixNanos(0))
            .with_name("ATO")
            .with_order_types([
                NamedOrderType::new(ato, "ATO"),
                NamedOrderType::new(OrderType::LIMIT, "LO"),
            ])
    }

    #[test]
    fn a_session_lets_through_the_types_it_takes() {
        let ato = NewOrder::new(
            InstrumentId::new("sim", "AAA"),
            Side::Buy,
            d("100"),
            OrderType::new(OrderKind::Market, TimeInForce::AtOpen),
            None,
        );

        assert_eq!(check_session(&ato, Some(&ato_session())), Ok(()));
        assert_eq!(check_session(&order("100", None), None), Ok(()));
    }

    #[test]
    fn a_session_refuses_other_types_and_says_what_it_takes() {
        let reject = check_session(&order("100", None), Some(&ato_session())).unwrap_err();

        assert_eq!(
            reject.to_string(),
            "HOSE is in ATO and takes only ATO, LO now"
        );
    }

    #[test]
    fn a_closed_market_takes_nothing() {
        let closed = SessionStatus::new("HNX", SessionPhase::Closed, UnixNanos(0));

        let reject = check_session(&order("100", Some("5")), Some(&closed)).unwrap_err();

        assert_eq!(
            reject.to_string(),
            "HNX is in a closed session and takes no orders now"
        );
    }

    fn no_short_selling() -> InstrumentInfo {
        let ticks = TickTable::flat(d("100")).unwrap();
        InstrumentInfo::new(InstrumentId::new("sim", "AAA"), d("100"), ticks)
            .with_short_selling(false)
    }

    fn sell(quantity: &str) -> NewOrder {
        NewOrder::market(InstrumentId::new("sim", "AAA"), Side::Sell, d(quantity))
    }

    fn holding(quantity: &str, settled: &str) -> Position {
        Position::new(InstrumentId::new("sim", "AAA"), d(quantity), d("62500"))
            .with_settled(d(settled))
    }

    #[test]
    fn a_sell_takes_only_settled_shares_not_already_in_working_sells() {
        let info = no_short_selling();
        let position = holding("500", "300");

        assert_eq!(
            check_sellable(&sell("200"), Some(&info), Some(&position), d("100")),
            Ok(())
        );
        let reject =
            check_sellable(&sell("300"), Some(&info), Some(&position), d("100")).unwrap_err();
        assert_eq!(
            reject.to_string(),
            "only 200 can be sold now: 500 held, 200 not settled yet, 100 in working sell orders"
        );
    }

    #[test]
    fn nothing_held_means_nothing_to_sell_where_short_selling_is_forbidden() {
        let reject = check_sellable(&sell("100"), Some(&no_short_selling()), None, d("0"));

        assert!(matches!(
            reject,
            Err(RiskReject::NotEnoughToSell { sellable, .. }) if sellable.is_zero()
        ));
    }

    #[test]
    fn a_market_that_allows_short_selling_or_a_buy_is_not_checked() {
        let ticks = TickTable::flat(d("0.01")).unwrap();
        let shortable = InstrumentInfo::new(InstrumentId::new("sim", "AAA"), d("1"), ticks);

        assert_eq!(
            check_sellable(&sell("100"), Some(&shortable), None, d("0")),
            Ok(())
        );
        assert_eq!(check_sellable(&sell("100"), None, None, d("0")), Ok(()));
        assert_eq!(
            check_sellable(&order("100", None), Some(&no_short_selling()), None, d("0")),
            Ok(())
        );
    }

    fn odd_lots_as_limits() -> InstrumentInfo {
        let ticks = TickTable::flat(d("100")).unwrap();
        InstrumentInfo::new(InstrumentId::new("sim", "AAA"), d("100"), ticks)
            .with_odd_lot_types([OrderType::LIMIT])
    }

    #[test]
    fn an_odd_lot_passes_as_a_type_the_market_takes_odd_lots_as() {
        let info = odd_lots_as_limits();

        assert_eq!(
            check_market_rules(&order("50", Some("62500")), Some(&info), None),
            Ok(())
        );
        assert_eq!(
            check_market_rules(&order("50", None), Some(&info), None)
                .unwrap_err()
                .to_string(),
            "an odd lot (fewer than 100) can only be a limit order for the day"
        );
    }

    #[test]
    fn a_mixed_quantity_is_neither_lots_nor_an_odd_lot() {
        assert_eq!(
            check_market_rules(
                &order("150", Some("62500")),
                Some(&odd_lots_as_limits()),
                None
            )
            .unwrap_err()
            .to_string(),
            "quantity must be whole lots of 100, or an odd lot below 100"
        );
    }
}
