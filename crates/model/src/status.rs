use crate::{InstrumentId, UnixNanos};

/// Whether one instrument trades now, as its venue says. A whole market's
/// day is a [`SessionStatus`](crate::SessionStatus); this is one
/// instrument's departure from it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum TradingStatus {
    /// Trading as its market does.
    Trading,
    /// Stopped for a while, to resume: a volatility pause, a halt pending
    /// news, a circuit breaker.
    Halted,
    /// Stopped until the venue or a regulator says otherwise.
    Suspended,
}

/// One instrument's status on its venue: whether it trades, whether the
/// venue takes orders for it while it does not, and whether short sales in
/// it are restricted. An adapter publishes one whenever any of it changes
/// (ADR 0026); an instrument with none published trades as its market does.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct InstrumentStatus {
    pub instrument: InstrumentId,
    pub trading: TradingStatus,
    /// While not trading, whether the venue still takes new orders, to
    /// queue for when it resumes. Taken as true while trading.
    pub takes_orders: bool,
    /// Short sales must pass a price test, as under a short-sale
    /// restriction: a sell beyond the holding must carry a limit price.
    pub short_sale_restricted: bool,
    /// The venue's words for the status, shown as written: why it halted,
    /// or a notice it trades under (a warning, a special listing).
    pub reason: Option<String>,
    pub ts_event: UnixNanos,
}

impl InstrumentStatus {
    /// `instrument` in `trading`, taking no orders while not trading, with
    /// no short-sale restriction and no reason.
    pub fn new(instrument: InstrumentId, trading: TradingStatus, ts_event: UnixNanos) -> Self {
        Self {
            instrument,
            trading,
            takes_orders: false,
            short_sale_restricted: false,
            reason: None,
            ts_event,
        }
    }

    pub fn taking_orders(self) -> Self {
        Self {
            takes_orders: true,
            ..self
        }
    }

    pub fn with_short_sale_restricted(self, restricted: bool) -> Self {
        Self {
            short_sale_restricted: restricted,
            ..self
        }
    }

    pub fn with_reason(self, reason: impl Into<String>) -> Self {
        Self {
            reason: Some(reason.into()),
            ..self
        }
    }

    /// Whether the venue takes new orders for it now.
    pub fn takes_new_orders(&self) -> bool {
        self.trading == TradingStatus::Trading || self.takes_orders
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(trading: TradingStatus) -> InstrumentStatus {
        InstrumentStatus::new(InstrumentId::new("sim", "AAA"), trading, UnixNanos(1))
    }

    #[test]
    fn an_instrument_takes_orders_while_it_trades_or_when_the_venue_queues_them() {
        assert!(status(TradingStatus::Trading).takes_new_orders());
        assert!(!status(TradingStatus::Halted).takes_new_orders());
        assert!(!status(TradingStatus::Suspended).takes_new_orders());
        assert!(status(TradingStatus::Halted)
            .taking_orders()
            .takes_new_orders());
    }
}
