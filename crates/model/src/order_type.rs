/// How an order is priced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum OrderKind {
    /// At its limit price or better.
    Limit,
    /// At whatever the market gives.
    Market,
    /// At the market; whatever does not fill at once rests as a limit order
    /// at a price the venue sets (on Vietnam's exchanges, one tick past the
    /// last match).
    MarketToLimit,
}

impl OrderKind {
    /// Whether an order of this kind carries a limit price.
    pub fn is_priced(self) -> bool {
        matches!(self, Self::Limit)
    }
}

/// How long an order stays, and which matching it is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum TimeInForce {
    /// Until it fills, is cancelled, or the trading day ends.
    Day,
    /// Fills what it can at once; the venue cancels the rest.
    ImmediateOrCancel,
    /// Fills in full at once, or the venue cancels all of it.
    FillOrKill,
    /// Only in the opening auction; what does not fill there is cancelled.
    AtOpen,
    /// Only in the closing auction; what does not fill there is cancelled.
    AtClose,
    /// Only in the session after the close, at the closing price.
    AfterClose,
}

/// What kind of order, independent of any venue's name for it. Vietnam's LO
/// is `Limit` for the `Day`, ATO is `Market` `AtOpen`, MAK is `Market`
/// `ImmediateOrCancel`; a US market-on-close is `Market` `AtClose`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct OrderType {
    pub kind: OrderKind,
    pub time_in_force: TimeInForce,
}

impl OrderType {
    /// A limit order for the day: what every venue takes.
    pub const LIMIT: Self = Self::new(OrderKind::Limit, TimeInForce::Day);
    /// A market order for the day.
    pub const MARKET: Self = Self::new(OrderKind::Market, TimeInForce::Day);

    pub const fn new(kind: OrderKind, time_in_force: TimeInForce) -> Self {
        Self {
            kind,
            time_in_force,
        }
    }
}

/// An order type as a venue offers it, under the name its traders know it
/// by ("LO", "ATO", "MOK").
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct NamedOrderType {
    pub order_type: OrderType,
    pub name: String,
}

impl NamedOrderType {
    pub fn new(order_type: OrderType, name: impl Into<String>) -> Self {
        Self {
            order_type,
            name: name.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_limit_order_carries_a_price() {
        assert!(OrderType::LIMIT.kind.is_priced());
        assert!(!OrderType::MARKET.kind.is_priced());
        assert!(!OrderKind::MarketToLimit.is_priced());
    }
}
