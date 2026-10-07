use crate::{NamedOrderType, OrderType, UnixNanos};

/// Where a market is in its trading day. Venues differ in which phases they
/// have and when; the adapter says which one is current.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SessionPhase {
    /// Before the day's trading.
    PreOpen,
    /// Orders collect, then match at one price when it ends.
    OpeningAuction,
    /// Orders match as they arrive.
    Continuous,
    /// A pause in the day, such as a lunch break.
    Break,
    /// Orders collect, then match at the closing price when it ends.
    ClosingAuction,
    /// After the closing auction: trading at the closing price, or
    /// negotiated deals only.
    AfterClose,
    /// No trading until the next day.
    Closed,
}

/// What one market is doing now, and the order types it takes in this
/// phase. `market` names it as [`InstrumentInfo::exchange`] does, so an
/// instrument's orders are checked against its own market's session.
///
/// [`InstrumentInfo::exchange`]: crate::InstrumentInfo::exchange
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct SessionStatus {
    pub market: String,
    pub phase: SessionPhase,
    /// The venue's own name for the phase, e.g. "ATO".
    pub name: Option<String>,
    /// What the market takes now. Empty: no orders at all.
    pub order_types: Vec<NamedOrderType>,
    pub ts_event: UnixNanos,
}

impl SessionStatus {
    pub fn new(market: impl Into<String>, phase: SessionPhase, ts_event: UnixNanos) -> Self {
        Self {
            market: market.into(),
            phase,
            name: None,
            order_types: Vec::new(),
            ts_event,
        }
    }

    pub fn with_name(self, name: impl Into<String>) -> Self {
        Self {
            name: Some(name.into()),
            ..self
        }
    }

    pub fn with_order_types(self, order_types: impl IntoIterator<Item = NamedOrderType>) -> Self {
        Self {
            order_types: order_types.into_iter().collect(),
            ..self
        }
    }

    /// The venue's entry for `order_type`, if the market takes it now.
    pub fn accepts(&self, order_type: OrderType) -> Option<&NamedOrderType> {
        self.order_types
            .iter()
            .find(|offered| offered.order_type == order_type)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{OrderKind, TimeInForce};

    #[test]
    fn a_session_takes_the_order_types_it_lists() {
        let ato = OrderType::new(OrderKind::Market, TimeInForce::AtOpen);
        let session = SessionStatus::new("HOSE", SessionPhase::OpeningAuction, UnixNanos(1))
            .with_name("ATO")
            .with_order_types([
                NamedOrderType::new(ato, "ATO"),
                NamedOrderType::new(OrderType::LIMIT, "LO"),
            ]);

        assert_eq!(session.accepts(ato).map(|t| t.name.as_str()), Some("ATO"));
        assert_eq!(session.accepts(OrderType::MARKET), None);
        assert_eq!(session.name.as_deref(), Some("ATO"));
    }
}
