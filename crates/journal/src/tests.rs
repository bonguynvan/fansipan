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
        }
    );
    assert_eq!(again.damaged, None);
    assert_eq!(
        Journal::open(&folder, "2026-10-20").unwrap().restored,
        Restored::default()
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
    assert_eq!(fs::read(&aside).unwrap(), br#"{"order":{"id":2,"#.to_vec());
    drop(again);
    let third = Journal::open(&folder, "2026-10-19").unwrap();
    assert_eq!(
        third.damaged, None,
        "the journal was cut back to its good lines"
    );
    assert_eq!(third.restored.orders.len(), 1);
    let _ = fs::remove_dir_all(&folder);
}

#[test]
fn a_line_that_does_not_parse_sets_it_and_everything_after_aside() {
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

    assert_eq!(
        again.restored.trading, None,
        "nothing after the bad line is applied"
    );
    assert_eq!(again.restored.orders.len(), 1);
    let aside = fs::read_to_string(again.damaged.clone().unwrap()).unwrap();
    assert!(aside.starts_with("{\"trading\":\"sideways\"}\n{\"trading\":\"halted\"}"));
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
