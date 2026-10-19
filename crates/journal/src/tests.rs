use std::sync::atomic::{AtomicU32, Ordering};

use execution_engine::{ConditionalId, ConditionalStatus, Offset, Then, Trigger};
use model::{
    ClientOrderId, Decimal, FillId, InstrumentId, Order, OrderKind, OrderStatus, OrderType, Side,
    TimeInForce, VenueOrderId,
};

use super::*;

fn d(raw: &str) -> Decimal {
    raw.parse().expect("test decimal")
}

/// A fresh folder under the system's temporary folder.
fn scratch() -> PathBuf {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let dir = std::env::temp_dir().join(format!(
        "fansipan-journal-test-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::SeqCst)
    ));
    let _ = fs::remove_dir_all(&dir);
    dir
}

fn order(id: u64, status: OrderStatus, filled: &str) -> OrderRecord {
    OrderRecord {
        order: Order {
            id: ClientOrderId(id),
            venue_order_id: Some(VenueOrderId(format!("V{id}"))),
            instrument: InstrumentId::new("sim", "AAA"),
            side: Side::Buy,
            quantity: d("100"),
            order_type: OrderType::new(OrderKind::Limit, TimeInForce::Day),
            limit_price: Some(d("25.05")),
            status,
            filled_quantity: d(filled),
            avg_fill_price: (d(filled) > Decimal::ZERO).then(|| d("25.0500")),
            reason: None,
        },
        applied_fills: vec![FillId("f1".into())],
        amending_to: None,
    }
}

fn stop(id: u64) -> Conditional {
    Conditional {
        id: ConditionalId(id),
        instrument: InstrumentId::new("sim", "AAA"),
        side: Side::Sell,
        quantity: d("100"),
        trigger: Trigger::Trailing(Offset::Percent(d("2.5"))),
        then: Then::Limit(d("24")),
        status: ConditionalStatus::Fired(ClientOrderId(9)),
        best: Some(d("26")),
        level: Some(d("25.35")),
        blocked: Some("trading is halted in this terminal".into()),
        after: Some(ClientOrderId(1)),
        oco: Some(ConditionalId(id + 1)),
        problem: None,
    }
}

#[test]
fn dates_are_utc_calendar_days() {
    assert_eq!(utc_date(0), "1970-01-01");
    assert_eq!(utc_date(1_792_368_000), "2026-10-19");
    assert_eq!(utc_date(1_792_368_000 - 1), "2026-10-18");
    assert_eq!(utc_date(1_709_164_800), "2024-02-29");
    assert_eq!(utc_date(951_868_799), "2000-02-29");
}

#[test]
fn an_accounts_folder_is_stable_and_never_names_the_account() {
    let base = Path::new("journals");
    let folder = account_folder(base, "alpaca", "PA-12345");

    assert_eq!(folder, account_folder(base, "alpaca", "PA-12345"));
    assert_ne!(folder, account_folder(base, "alpaca", "PA-12346"));
    assert_ne!(folder, account_folder(base, "other", "PA-12345"));
    assert!(!folder.to_string_lossy().contains("12345"));
}

#[test]
fn a_session_written_reads_back_exactly_with_the_last_record_winning() {
    let folder = scratch();
    let mut opened = Journal::open(&folder, "2026-10-19").unwrap();
    assert_eq!(opened.restored, Restored::default());
    let records = [
        Record::order(&order(1, OrderStatus::Accepted, "0")).unwrap(),
        Record::order(&order(2, OrderStatus::Cancelled, "0")).unwrap(),
        Record::conditional(&stop(5)).unwrap(),
        Record::trading(TradingState::Halted),
        Record::order(&order(1, OrderStatus::PartiallyFilled, "40")).unwrap(),
        Record::trading(TradingState::Reducing),
    ];
    opened.journal.append(&records).unwrap();
    drop(opened);

    let again = Journal::open(&folder, "2026-10-19").unwrap();

    assert_eq!(
        again.restored,
        Restored {
            orders: vec![
                order(1, OrderStatus::PartiallyFilled, "40"),
                order(2, OrderStatus::Cancelled, "0")
            ],
            conditionals: vec![stop(5)],
            trading: Some(TradingState::Reducing),
            next_order_id: None,
        }
    );
    assert_eq!(again.damaged, None);
    drop(again);
    assert_eq!(
        Journal::open(&folder, "2026-10-20").unwrap().restored,
        Restored {
            orders: vec![order(1, OrderStatus::PartiallyFilled, "40")],
            conditionals: vec![],
            trading: Some(TradingState::Reducing),
            next_order_id: Some(3),
        },
        "the next day starts with what was live; the fired stop is spent"
    );
    let _ = fs::remove_dir_all(&folder);
}

#[test]
fn an_unfinished_last_line_is_set_aside_and_the_rest_kept() {
    let folder = scratch();
    let mut opened = Journal::open(&folder, "2026-10-19").unwrap();
    opened
        .journal
        .append(&[Record::order(&order(1, OrderStatus::Accepted, "0")).unwrap()])
        .unwrap();
    let path = opened.journal.path().to_path_buf();
    drop(opened);
    let mut file = OpenOptions::new().append(true).open(&path).unwrap();
    file.write_all(br#"{"order":{"id":2,"#).unwrap();
    drop(file);

    let again = Journal::open(&folder, "2026-10-19").unwrap();

    assert_eq!(
        again.restored.orders,
        vec![order(1, OrderStatus::Accepted, "0")]
    );
    let aside = again.damaged.clone().expect("the torn line set aside");
    assert_eq!(
        fs::read_to_string(&aside).unwrap(),
        "{\"order\":{\"id\":2,\n"
    );
    drop(again);
    let third = Journal::open(&folder, "2026-10-19").unwrap();
    assert_eq!(
        third.damaged, None,
        "the journal was rewritten with its good lines"
    );
    assert_eq!(third.restored.orders.len(), 1);
    let _ = fs::remove_dir_all(&folder);
}

#[test]
fn a_line_that_cannot_be_read_is_set_aside_and_the_lines_after_it_still_count() {
    let folder = scratch();
    let mut opened = Journal::open(&folder, "2026-10-19").unwrap();
    let path = opened.journal.path().to_path_buf();
    opened
        .journal
        .append(&[Record::order(&order(1, OrderStatus::Accepted, "0")).unwrap()])
        .unwrap();
    drop(opened);
    let mut file = OpenOptions::new().append(true).open(&path).unwrap();
    file.write_all(b"{\"trading\":\"sideways\"}\n").unwrap();
    file.write_all(b"{\"trading\":\"halted\"}\n").unwrap();
    drop(file);

    let again = Journal::open(&folder, "2026-10-19").unwrap();

    assert_eq!(again.restored.trading, Some(TradingState::Halted));
    assert_eq!(again.restored.orders.len(), 1);
    let aside = fs::read_to_string(again.damaged.clone().unwrap()).unwrap();
    assert_eq!(aside, "{\"trading\":\"sideways\"}\n");
    let _ = fs::remove_dir_all(&folder);
}

#[test]
fn a_header_that_names_no_format_is_set_aside_not_taken_for_a_newer_one() {
    let folder = scratch();
    fs::create_dir_all(&folder).unwrap();
    fs::write(
        folder.join("2026-10-19.jsonl"),
        b"garbage\n{\"trading\":\"halted\"}\n",
    )
    .unwrap();

    let opened = Journal::open(&folder, "2026-10-19").unwrap();

    assert!(opened.damaged.is_some());
    assert_eq!(opened.restored.trading, Some(TradingState::Halted));
    let _ = fs::remove_dir_all(&folder);
}

#[test]
fn a_journal_from_a_newer_version_is_left_as_it_is() {
    let folder = scratch();
    fs::create_dir_all(&folder).unwrap();
    let path = folder.join("2026-10-19.jsonl");
    let newer = b"{\"journal\":2,\"date\":\"2026-10-19\"}\n{\"whatever\":1}\n";
    fs::write(&path, newer).unwrap();

    assert!(matches!(
        Journal::open(&folder, "2026-10-19"),
        Err(JournalError::Format(2))
    ));
    assert_eq!(fs::read(&path).unwrap(), newer.to_vec());
    let _ = fs::remove_dir_all(&folder);
}

#[test]
fn every_order_type_and_status_reads_back() {
    let kinds = [
        OrderKind::Limit,
        OrderKind::Market,
        OrderKind::MarketToLimit,
    ];
    let forces = [
        TimeInForce::Day,
        TimeInForce::ImmediateOrCancel,
        TimeInForce::FillOrKill,
        TimeInForce::AtOpen,
        TimeInForce::AtClose,
        TimeInForce::AfterClose,
    ];
    let statuses = [
        OrderStatus::Submitted,
        OrderStatus::Accepted,
        OrderStatus::PartiallyFilled,
        OrderStatus::Filled,
        OrderStatus::PendingCancel,
        OrderStatus::PendingAmend,
        OrderStatus::Cancelled,
        OrderStatus::Rejected,
        OrderStatus::Denied,
    ];
    for kind in kinds {
        for tif in forces {
            for status in statuses {
                let mut record = order(1, status, "0");
                record.order.order_type = OrderType::new(kind, tif);
                let Record::Order(line) = Record::order(&record).expect("every shape records")
                else {
                    unreachable!("an order records as one");
                };
                assert_eq!(line.to_record().unwrap(), record);
            }
        }
    }
}

#[test]
fn the_highest_reservation_wins() {
    let folder = scratch();
    let mut opened = Journal::open(&folder, "2026-10-19").unwrap();
    opened
        .journal
        .append(&[
            Record::reserved(1_001),
            Record::reserved(3_001),
            Record::reserved(2_001),
        ])
        .unwrap();
    drop(opened);

    assert_eq!(
        Journal::open(&folder, "2026-10-19")
            .unwrap()
            .restored
            .next_order_id,
        Some(3_001)
    );
    let _ = fs::remove_dir_all(&folder);
}

#[test]
fn a_new_day_starts_from_what_was_live_in_the_last_one() {
    let folder = scratch();
    let mut monday = Journal::open(&folder, "2026-10-19").unwrap();
    let waiting = Conditional {
        status: ConditionalStatus::Waiting,
        ..stop(5)
    };
    monday
        .journal
        .append(&[
            Record::order(&order(1, OrderStatus::Accepted, "0")).unwrap(),
            Record::order(&order(40, OrderStatus::Filled, "100")).unwrap(),
            Record::conditional(&waiting).unwrap(),
            Record::conditional(&stop(6)).unwrap(),
            Record::trading(TradingState::Halted),
        ])
        .unwrap();
    drop(monday);

    let tuesday = Journal::open(&folder, "2026-10-21").unwrap();

    assert_eq!(
        tuesday.restored,
        Restored {
            orders: vec![order(1, OrderStatus::Accepted, "0")],
            conditionals: vec![waiting],
            trading: Some(TradingState::Halted),
            next_order_id: Some(41),
        },
        "only what is live comes over, with ids past every order, finished ones too"
    );
    drop(tuesday);
    let again = Journal::open(&folder, "2026-10-21").unwrap();
    assert_eq!(
        again.restored.next_order_id,
        Some(41),
        "and it is in the new day's file"
    );
    let _ = fs::remove_dir_all(&folder);
}

#[test]
fn one_terminal_at_a_time_keeps_an_accounts_journal() {
    let folder = scratch();
    let held = AccountLock::acquire(&folder).unwrap();

    assert!(matches!(
        AccountLock::acquire(&folder),
        Err(JournalError::Locked)
    ));
    drop(held);
    assert!(AccountLock::acquire(&folder).is_ok());
    let _ = fs::remove_dir_all(&folder);
}

#[test]
fn every_conditional_shape_reads_back() {
    let shapes = [
        (
            Trigger::AtOrBelow(d("9.5")),
            Then::Marketable,
            ConditionalStatus::Waiting,
        ),
        (
            Trigger::AtOrAbove(d("11")),
            Then::Limit(d("11")),
            ConditionalStatus::Pending,
        ),
        (
            Trigger::Trailing(Offset::Amount(d("0.5"))),
            Then::Marketable,
            ConditionalStatus::Cancelled,
        ),
    ];
    for (trigger, then, status) in shapes {
        let held = Conditional {
            trigger,
            then,
            status,
            best: None,
            level: None,
            blocked: None,
            after: None,
            oco: None,
            ..stop(3)
        };
        let Record::Conditional(line) = Record::conditional(&held).unwrap() else {
            unreachable!("a conditional records as one");
        };
        assert_eq!(line.to_conditional().unwrap(), held);
    }
}
