//! Commands the venue has not answered: an order sent, or a
//! cancel or an amendment asked for, with no word from the venue for a
//! while. The engine asks the venue where such an order stands, through the
//! recovery a restart uses, and the answer settles it in the lifecycle.

use std::time::{Duration, Instant};

use adapter_api::AdapterError;
use model::{ClientOrderId, OrderStatus};

/// How many times one wait is asked about. After that the order stays
/// marked unanswered until the venue speaks of it, after a reconnect if
/// not before.
pub const ASKS: u32 = 3;

/// How many asks a cancel waits before a venue that still holds the order
/// working is taken to have refused it: the first answer may come while
/// the venue has the cancel queued.
pub const CANCEL_ASKS: u32 = 2;

/// What one round of asking did.
#[derive(Debug, Default)]
pub struct Asked {
    /// The orders asked about, whose marks may have changed.
    pub orders: Vec<ClientOrderId>,
    /// Why the venue could not be asked, if it could not.
    pub problem: Option<AdapterError>,
}

/// Whether an order in `status` waits on the venue's answer to a command.
pub(crate) fn awaits_answer(status: OrderStatus) -> bool {
    matches!(
        status,
        OrderStatus::Submitted | OrderStatus::PendingCancel | OrderStatus::PendingAmend
    )
}

/// One order's wait for an answer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Waiting {
    /// The status it waits in; any other ends the wait.
    pub status: OrderStatus,
    /// When it was last asked about, or first seen waiting.
    pub since: Instant,
    pub asked: u32,
}

impl Waiting {
    pub(crate) fn new(status: OrderStatus, now: Instant) -> Self {
        Self {
            status,
            since: now,
            asked: 0,
        }
    }

    /// Whether to ask about it at `now`: `after` has passed since it began
    /// waiting or was last asked about, and it has asks left.
    pub(crate) fn due(&self, now: Instant, after: Duration) -> bool {
        self.asked < ASKS && now.saturating_duration_since(self.since) >= after
    }

    pub(crate) fn asked_at(self, now: Instant) -> Self {
        Self {
            since: now,
            asked: self.asked + 1,
            ..self
        }
    }

    /// Whether an order now in `status` has been asked about in this wait.
    pub(crate) fn unanswered(&self, status: OrderStatus) -> bool {
        self.asked > 0 && self.status == status
    }

    /// Whether a cancel has waited long enough that the venue still
    /// holding the order working means the cancel did not take.
    pub(crate) fn cancel_waited_out(&self) -> bool {
        self.status == OrderStatus::PendingCancel && self.asked >= CANCEL_ASKS
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const AFTER: Duration = Duration::from_secs(15);

    #[test]
    fn a_wait_is_due_once_the_time_has_passed_and_not_before() {
        let start = Instant::now();
        let waiting = Waiting::new(OrderStatus::Submitted, start);

        assert!(!waiting.due(start + AFTER - Duration::from_millis(1), AFTER));
        assert!(waiting.due(start + AFTER, AFTER));
    }

    #[test]
    fn each_ask_starts_the_time_again_until_the_asks_run_out() {
        let start = Instant::now();
        let mut waiting = Waiting::new(OrderStatus::PendingCancel, start);
        let mut now = start;
        for _ in 0..ASKS {
            now += AFTER;
            assert!(waiting.due(now, AFTER));
            waiting = waiting.asked_at(now);
            assert!(!waiting.due(now, AFTER), "not again at once");
        }

        assert!(!waiting.due(now + AFTER * 10, AFTER), "asks run out");
    }

    #[test]
    fn a_wait_is_unanswered_only_once_asked_and_while_in_its_status() {
        let start = Instant::now();
        let waiting = Waiting::new(OrderStatus::Submitted, start);
        assert!(!waiting.unanswered(OrderStatus::Submitted));

        let asked = waiting.asked_at(start + AFTER);

        assert!(asked.unanswered(OrderStatus::Submitted));
        assert!(!asked.unanswered(OrderStatus::Accepted), "answered");
    }

    #[test]
    fn a_cancel_waits_out_only_after_its_asks() {
        let start = Instant::now();
        let mut waiting = Waiting::new(OrderStatus::PendingCancel, start);
        for _ in 0..CANCEL_ASKS {
            assert!(!waiting.cancel_waited_out());
            waiting = waiting.asked_at(start);
        }

        assert!(waiting.cancel_waited_out());
        assert!(
            !Waiting {
                status: OrderStatus::Submitted,
                ..waiting
            }
            .cancel_waited_out(),
            "only a cancel"
        );
    }

    #[test]
    fn only_a_command_in_flight_awaits_an_answer() {
        for status in [
            OrderStatus::Submitted,
            OrderStatus::PendingCancel,
            OrderStatus::PendingAmend,
        ] {
            assert!(awaits_answer(status), "{status:?}");
        }
        for status in [
            OrderStatus::Accepted,
            OrderStatus::PartiallyFilled,
            OrderStatus::Filled,
            OrderStatus::Cancelled,
        ] {
            assert!(!awaits_answer(status), "{status:?}");
        }
    }
}
