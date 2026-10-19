//! The shell's side of the journal (ADR 0023). At start it takes the
//! account's lock, opens the journal for today, restores the engine from
//! it, and asks the venue about restored orders still open. What the trader
//! does is recorded before the command returns, and order ids are reserved
//! before an order may use them; what the venue reports is recorded at the
//! next flush. Records are built under the engine's lock and written after
//! it is released, so a slow disk never holds up the engine. A write that
//! fails is reported once, and the next one records everything again.

use std::path::{Path, PathBuf};

use execution_engine::{ConditionalStatus, ExecutionEngine};
use journal::{account_folder, utc_date, AccountLock, Journal, Record};
use model::ClientOrderId;
use serde::Serialize;

/// Ids reserved past the next one, beyond one for each conditional order
/// that could still fire, whenever a reservation is renewed.
const RESERVE_BLOCK: u64 = 1_000;
/// Renewed once fewer than this many reserved ids would be left.
const RESERVE_MARGIN: u64 = 100;

/// What the screens are told about the journal.
#[derive(Serialize, Clone, Debug, Default, PartialEq)]
pub struct JournalDto {
    /// Whether a journal is kept: the adapter named its account.
    pub kept: bool,
    pub restored_orders: usize,
    pub restored_conditionals: usize,
    /// Restored orders still open that the venue has not spoken of yet.
    pub unconfirmed: usize,
    /// Whether the venue was asked about them and can answer.
    pub venue_asked: bool,
    /// Where lines that could not be read were set aside.
    pub damaged: Option<String>,
    /// Why the journal could not be opened or written, if it could not.
    pub error: Option<String>,
}

/// An account's journal, open for today, and the lock that makes it ours.
pub struct Kept {
    _lock: AccountLock,
    journal: Journal,
    folder: PathBuf,
    date: String,
    /// The first order id not yet reserved.
    reserved: u64,
    /// Whether the last write failed, so the next records everything.
    resync: bool,
    /// The last problem told to the screens, so each is told once.
    told: Option<String>,
}

/// Records built under the engine's lock, to write after it is released,
/// and what could not be recorded.
pub struct Built {
    records: Vec<Record>,
    problems: Vec<String>,
}

impl Built {
    fn push(&mut self, record: Result<Record, journal::RecordError>) {
        match record {
            Ok(record) => self.records.push(record),
            Err(e) => self.problems.push(e.to_string()),
        }
    }
}

/// Opens the account's journal for the day `now_seconds` falls on and
/// rebuilds the engine from it. `None` when the adapter names no account,
/// or another terminal keeps this account's journal.
pub fn open(
    exec: &mut ExecutionEngine,
    base: &Path,
    venue: &str,
    now_seconds: u64,
) -> (Option<Kept>, JournalDto) {
    let Some(account) = exec.account_id() else {
        return (None, JournalDto::default());
    };
    let folder = account_folder(base, venue, &account);
    // Its name is a hash of the account, which the log does not carry.
    crate::logbook::hide(&folder, "<account journal>");
    let failed = |e: journal::JournalError| JournalDto {
        error: Some(e.to_string()),
        ..JournalDto::default()
    };
    let lock = match AccountLock::acquire(&folder) {
        Ok(lock) => lock,
        Err(e) => return (None, failed(e)),
    };
    let date = utc_date(now_seconds);
    let opened = match Journal::open(&folder, &date) {
        Ok(opened) => opened,
        Err(e) => return (None, failed(e)),
    };
    let mut report = JournalDto {
        kept: true,
        restored_orders: opened.restored.orders.len(),
        restored_conditionals: opened.restored.conditionals.len(),
        damaged: opened.damaged.map(|path| path.display().to_string()),
        ..JournalDto::default()
    };
    let reserved = opened.restored.next_order_id.unwrap_or(1);
    if let Err(e) = exec.restore(opened.restored) {
        report.error = Some(e.to_string());
    }
    match exec.recover_unconfirmed() {
        Ok(asked) => report.venue_asked = asked,
        Err(e) => {
            report.error = Some(format!(
                "the venue could not be asked about restored orders: {e}"
            ));
        }
    }
    report.unconfirmed = exec
        .orders()
        .filter(|order| exec.is_unconfirmed(order.id))
        .count();
    let mut kept = Kept {
        _lock: lock,
        journal: opened.journal,
        folder,
        date,
        reserved,
        resync: false,
        told: None,
    };
    // Reserve before anything can be sent.
    let built = kept.build(exec, &[], &[], false);
    if let Some(e) = kept.write(built, now_seconds) {
        report.error = Some(e);
    }
    (Some(kept), report)
}

impl Kept {
    /// Whether the last write failed, and the next must record everything.
    pub fn resyncing(&self) -> bool {
        self.resync
    }

    /// The records for the orders and conditional orders named, the
    /// trader's switch if `trading`, and a reservation of order ids if one
    /// is due: enough that every conditional order still live could fire
    /// and the next orders sent stay within it. After a failed write, the
    /// records for everything the engine holds.
    pub fn build(
        &mut self,
        exec: &ExecutionEngine,
        orders: &[ClientOrderId],
        conditionals: &[u64],
        trading: bool,
    ) -> Built {
        let mut built = Built {
            records: Vec::new(),
            problems: Vec::new(),
        };
        let everything = self.resync;
        let live = exec
            .conditionals()
            .filter(|c| {
                matches!(
                    c.status,
                    ConditionalStatus::Pending | ConditionalStatus::Waiting
                )
            })
            .count() as u64;
        let next = exec.next_order_id();
        if everything || next.saturating_add(live).saturating_add(RESERVE_MARGIN) > self.reserved {
            self.reserved = next.saturating_add(live).saturating_add(RESERVE_BLOCK);
            built.records.push(Record::reserved(self.reserved));
        }
        for order in exec.orders() {
            if everything || orders.contains(&order.id) {
                if let Some(record) = exec.order_record(order.id) {
                    built.push(Record::order(&record));
                }
            }
        }
        for conditional in exec.conditionals() {
            if everything || conditionals.contains(&conditional.id.0) {
                built.push(Record::conditional(conditional));
            }
        }
        if everything || trading {
            built.records.push(Record::trading(exec.trading_state()));
        }
        built
    }

    /// Writes `built`, into the next day's file once the UTC date has
    /// turned (which opens with what was live in today's). Returns a
    /// problem the first time it happens: a write that failed, or a state
    /// this version cannot record; none while it goes on, or once all is
    /// well again.
    pub fn write(&mut self, built: Built, now_seconds: u64) -> Option<String> {
        let date = utc_date(now_seconds);
        let mut outcome = Ok(());
        let mut carried_damage = None;
        if date != self.date {
            match Journal::open(&self.folder, &date) {
                Ok(opened) => {
                    carried_damage = opened.damaged.map(|path| {
                        format!(
                            "lines of the journal that could not be read were set aside in {}",
                            path.display()
                        )
                    });
                    self.journal = opened.journal;
                    self.date = date;
                }
                Err(e) => outcome = Err(e.to_string()),
            }
        }
        if outcome.is_ok() {
            outcome = self
                .journal
                .append(&built.records)
                .map_err(|e| e.to_string());
        }
        let problem = match outcome {
            Ok(()) => {
                self.resync = false;
                built.problems.into_iter().next().or(carried_damage)
            }
            Err(e) => {
                self.resync = true;
                Some(e)
            }
        };
        match problem {
            None => {
                self.told = None;
                None
            }
            Some(problem) if self.told.as_deref() != Some(problem.as_str()) => {
                self.told = Some(problem.clone());
                Some(problem)
            }
            Some(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU32, Ordering};

    use adapter_api::{AdapterError, ExecutionClient, OrderRecovery};
    use bus::BusHandle;
    use execution_engine::{ConditionalRequest, RiskLimits, Then, Trigger};
    use model::{
        AccountState, CancelOrder, Decimal, InstrumentId, NewOrder, RecoverOrder, Side, SubmitOrder,
    };

    use super::*;

    /// A venue that names its account and answers about restored orders.
    struct Named(Option<&'static str>);

    impl ExecutionClient for Named {
        fn connect(&mut self, _bus: Box<dyn BusHandle>) -> Result<(), AdapterError> {
            Ok(())
        }
        fn submit_order(&mut self, _order: SubmitOrder) -> Result<(), AdapterError> {
            Ok(())
        }
        fn cancel_order(&mut self, _cancel: CancelOrder) -> Result<(), AdapterError> {
            Ok(())
        }
        fn account_state(&mut self) -> Result<AccountState, AdapterError> {
            Ok(AccountState::default())
        }
        fn disconnect(&mut self) -> Result<(), AdapterError> {
            Ok(())
        }
        fn account_id(&self) -> Option<String> {
            self.0.map(String::from)
        }
        fn recovery(&mut self) -> Option<&mut dyn OrderRecovery> {
            Some(self)
        }
    }

    impl OrderRecovery for Named {
        fn recover_orders(&mut self, _orders: Vec<RecoverOrder>) -> Result<(), AdapterError> {
            Ok(())
        }
    }

    fn engine(account: Option<&'static str>) -> ExecutionEngine {
        let limits = RiskLimits {
            max_order_quantity: Decimal::from(1_000),
            max_order_notional: Decimal::from(1_000_000),
        };
        ExecutionEngine::new(Box::new(Named(account)), limits)
    }

    fn scratch() -> PathBuf {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        std::env::temp_dir().join(format!(
            "fansipan-shell-journal-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ))
    }

    const MONDAY: u64 = 1_792_368_000 + 3_600;

    #[test]
    fn a_venue_that_names_no_account_keeps_no_journal() {
        let (kept, report) = open(&mut engine(None), &scratch(), "sim", MONDAY);

        assert!(kept.is_none());
        assert_eq!(report, JournalDto::default());
    }

    #[test]
    fn a_session_recorded_at_flushes_comes_back_in_a_new_engine() {
        let base = scratch();
        let mut before = engine(Some("A-1"));
        let (kept, report) = open(&mut before, &base, "sim", MONDAY);
        let mut kept = kept.expect("a named account keeps a journal");
        assert!(report.kept);
        let aaa = InstrumentId::new("sim", "AAA");
        let order = before.submit(NewOrder::limit(
            aaa.clone(),
            Side::Buy,
            Decimal::from(5),
            Decimal::from(10),
        ));
        let held = before
            .place_conditional(
                ConditionalRequest {
                    instrument: aaa,
                    side: Side::Sell,
                    quantity: Decimal::from(5),
                    trigger: Trigger::AtOrBelow(Decimal::from(9)),
                    then: Then::Marketable,
                },
                None,
            )
            .unwrap();
        let built = kept.build(&before, &[order.id], &[held.id.0], true);
        assert_eq!(kept.write(built, MONDAY), None);
        drop(kept);

        let mut after = engine(Some("A-1"));
        let (_, report) = open(&mut after, &base, "sim", MONDAY + 60);

        assert_eq!(
            (
                report.restored_orders,
                report.restored_conditionals,
                report.unconfirmed,
                report.venue_asked
            ),
            (1, 1, 1, true)
        );
        assert_eq!(after.orders().next(), Some(&order));
        assert_eq!(after.conditionals().next(), Some(&held));
        let (_, elsewhere) = open(&mut engine(Some("A-2")), &base, "sim", MONDAY);
        assert_eq!(
            elsewhere.restored_orders, 0,
            "another account's journal is its own"
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn a_second_terminal_on_the_same_account_keeps_no_journal_and_is_told_why() {
        let base = scratch();
        let (first, _) = open(&mut engine(Some("A-1")), &base, "sim", MONDAY);
        assert!(first.is_some());

        let (second, report) = open(&mut engine(Some("A-1")), &base, "sim", MONDAY);

        assert!(second.is_none());
        assert!(!report.kept);
        assert!(report.error.unwrap().contains("another terminal"));
        drop(first);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn ids_reserved_at_start_are_never_used_again_after_a_crash() {
        let base = scratch();
        let mut before = engine(Some("A-1"));
        let kept = open(&mut before, &base, "sim", MONDAY).0.unwrap();
        // An order goes out and the terminal dies before any flush.
        let sent = before.submit(NewOrder::limit(
            InstrumentId::new("sim", "AAA"),
            Side::Buy,
            Decimal::from(5),
            Decimal::from(10),
        ));
        drop(kept);

        let mut after = engine(Some("A-1"));
        let _ = open(&mut after, &base, "sim", MONDAY + 60);

        assert!(
            after.next_order_id() > sent.id.0,
            "the lost order's id is not reused"
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn a_new_day_starts_a_file_that_holds_everything() {
        let base = scratch();
        let mut before = engine(Some("A-1"));
        let mut kept = open(&mut before, &base, "sim", MONDAY).0.unwrap();
        let aaa = InstrumentId::new("sim", "AAA");
        let order = before.submit(NewOrder::limit(
            aaa,
            Side::Buy,
            Decimal::from(5),
            Decimal::from(10),
        ));
        let built = kept.build(&before, &[order.id], &[], false);
        assert_eq!(kept.write(built, MONDAY), None);
        drop(kept);

        let (_, tuesday) = open(&mut engine(Some("A-1")), &base, "sim", MONDAY + 86_400);
        assert_eq!(tuesday.restored_orders, 1);
        let _ = std::fs::remove_dir_all(&base);
    }
}
