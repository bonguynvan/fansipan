//! The shell's side of the journal (ADR 0023). At start it opens the
//! account's journal for today, restores the engine from it, and asks the
//! venue about restored orders still open. At every flush it records what
//! the batch names, read whole from the engine. On a new UTC date it starts
//! that day's file with everything the engine holds, so each day's file
//! stands alone.

use std::path::{Path, PathBuf};

use execution_engine::ExecutionEngine;
use journal::{account_folder, utc_date, Journal, Record};
use serde::Serialize;

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
    /// Where a damaged tail was set aside.
    pub damaged: Option<String>,
    /// Why the journal could not be opened or written, if it could not.
    pub error: Option<String>,
}

/// An open day's journal.
pub struct Kept {
    journal: Journal,
    folder: PathBuf,
    date: String,
    /// Whether the last write failed, so a failure is reported once.
    failing: bool,
}

/// Opens the account's journal for the day `now_seconds` falls on and
/// rebuilds the engine from it. `None` when the adapter names no account.
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
    let date = utc_date(now_seconds);
    let opened = match Journal::open(&folder, &date) {
        Ok(opened) => opened,
        Err(e) => {
            let report = JournalDto {
                error: Some(e.to_string()),
                ..JournalDto::default()
            };
            return (None, report);
        }
    };
    let restored_orders = opened.restored.orders.len();
    let restored_conditionals = opened.restored.conditionals.len();
    let mut report = JournalDto {
        kept: true,
        restored_orders,
        restored_conditionals,
        damaged: opened.damaged.map(|path| path.display().to_string()),
        ..JournalDto::default()
    };
    if let Err(e) = exec.restore(opened.restored) {
        report.error = Some(e.to_string());
    }
    match exec.recover_unconfirmed() {
        Ok(asked) => report.venue_asked = asked,
        Err(e) => {
            report.error = Some(format!(
                "the venue could not be asked about restored orders: {e}"
            ))
        }
    }
    report.unconfirmed = exec
        .orders()
        .filter(|order| exec.is_unconfirmed(order.id))
        .count();
    let kept = Kept {
        journal: opened.journal,
        folder,
        date,
        failing: false,
    };
    (Some(kept), report)
}

/// Every record the engine would need to rebuild what it holds now.
fn everything(exec: &ExecutionEngine) -> Result<Vec<Record>, String> {
    let orders = exec.orders().map(|order| order.id).collect::<Vec<_>>();
    let conditionals = exec.conditionals().map(|c| c.id.0).collect::<Vec<_>>();
    records(exec, &orders, &conditionals, true)
}

/// The records for `orders`, `conditionals`, and the switch if `trading`.
/// A state this version cannot record is an error, never skipped.
fn records(
    exec: &ExecutionEngine,
    orders: &[model::ClientOrderId],
    conditionals: &[u64],
    trading: bool,
) -> Result<Vec<Record>, String> {
    let mut out = Vec::new();
    for record in orders.iter().filter_map(|id| exec.order_record(*id)) {
        out.push(Record::order(&record).map_err(|e| e.to_string())?);
    }
    for conditional in exec
        .conditionals()
        .filter(|c| conditionals.contains(&c.id.0))
    {
        out.push(Record::conditional(conditional).map_err(|e| e.to_string())?);
    }
    if trading {
        out.push(Record::trading(exec.trading_state()));
    }
    Ok(out)
}

impl Kept {
    /// Records the orders and conditional orders a flush named, and the
    /// trader's switch if it moved. Returns a message the first time
    /// writing fails, and none while it keeps failing or once it works.
    pub fn record(
        &mut self,
        exec: &ExecutionEngine,
        orders: &[model::ClientOrderId],
        conditionals: &[u64],
        trading: bool,
        now_seconds: u64,
    ) -> Option<String> {
        let date = utc_date(now_seconds);
        let written = if date != self.date {
            self.next_day(exec, &date)
        } else {
            records(exec, orders, conditionals, trading)
                .and_then(|records| self.journal.append(&records).map_err(|e| e.to_string()))
        };
        match written {
            Ok(()) => {
                self.failing = false;
                None
            }
            Err(e) if !self.failing => {
                self.failing = true;
                Some(e)
            }
            Err(_) => None,
        }
    }

    fn next_day(&mut self, exec: &ExecutionEngine, date: &str) -> Result<(), String> {
        let mut opened = Journal::open(&self.folder, date).map_err(|e| e.to_string())?;
        opened
            .journal
            .append(&everything(exec)?)
            .map_err(|e| e.to_string())?;
        self.journal = opened.journal;
        self.date = date.to_string();
        Ok(())
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
        assert_eq!(
            kept.record(&before, &[order.id], &[held.id.0], true, MONDAY),
            None
        );

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
        assert_eq!(kept.record(&before, &[order.id], &[], false, MONDAY), None);

        // Past midnight, a flush that names nothing new still opens the day.
        assert_eq!(kept.record(&before, &[], &[], false, MONDAY + 86_400), None);

        let (_, tuesday) = open(&mut engine(Some("A-1")), &base, "sim", MONDAY + 86_400);
        assert_eq!(tuesday.restored_orders, 1);
        let _ = std::fs::remove_dir_all(&base);
    }
}
