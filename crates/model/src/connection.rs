use crate::UnixNanos;

/// Which of an adapter's connections a [`ConnectionStatus`] is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub enum Connection {
    /// The `DataClient`'s market-data feed.
    MarketData,
    /// The `ExecutionClient`'s link for orders and the account.
    Orders,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ConnectionState {
    /// Delivering everything.
    Up,
    /// Delivering, but losing some of it: messages that could not be read,
    /// a feed that has fallen behind.
    Degraded,
    /// Lost, and the adapter is bringing it back on its own.
    Reconnecting,
    /// Lost, and the adapter will not retry: refused keys, a closed account,
    /// or a disconnect core asked for.
    Down,
}

/// An adapter's report on one of its connections, published when it changes.
/// A connection nobody has reported on since `connect` succeeded is up.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ConnectionStatus {
    pub connection: Connection,
    pub state: ConnectionState,
    /// What happened, in the adapter's or the venue's words. Shown as written.
    pub detail: Option<String>,
    pub ts_event: UnixNanos,
}

impl ConnectionStatus {
    pub fn new(connection: Connection, state: ConnectionState, ts_event: UnixNanos) -> Self {
        Self {
            connection,
            state,
            detail: None,
            ts_event,
        }
    }

    pub fn with_detail(self, detail: impl Into<String>) -> Self {
        Self {
            detail: Some(detail.into()),
            ..self
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_status_starts_without_detail_and_can_be_given_one() {
        let status = ConnectionStatus::new(
            Connection::Orders,
            ConnectionState::Reconnecting,
            UnixNanos(7),
        );
        assert_eq!(status.detail, None);

        let explained = status.clone().with_detail("stream closed by the venue");

        assert_eq!(
            explained.detail.as_deref(),
            Some("stream closed by the venue")
        );
        assert_eq!(
            (explained.connection, explained.state, explained.ts_event),
            (status.connection, status.state, status.ts_event)
        );
    }
}
