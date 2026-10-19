//! The engine's records on disk, and the session rebuilt from them
//! (ADR 0023). One folder per account, named by a hash of it, and one file
//! per UTC date: a header line, then one record per line, each the whole
//! state of one thing, the last for each thing winning. A day's file opens
//! with what was still live in the latest earlier one. A line that cannot
//! be read is moved to a `.damaged` file beside the journal and skipped;
//! the rest is read. One process at a time keeps an account's journal.

mod record;

use std::collections::BTreeMap;
use std::fmt;
use std::fs::{self, File, OpenOptions, TryLockError};
use std::io::{self, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use execution_engine::{Conditional, ConditionalStatus, OrderRecord, Restored, TradingState};
use serde::{Deserialize, Serialize};

pub use record::{ConditionalLine, OrderLine, Record, RecordError};

/// The journal format this version writes and reads.
pub const FORMAT: u64 = 1;
const EXTENSION: &str = "jsonl";
const LOCK_FILE: &str = "lock";

#[derive(Debug)]
pub enum JournalError {
    Io(io::Error),
    /// A journal written by a newer version; it is left as it is.
    Format(u64),
    Record(RecordError),
    /// Another process keeps this account's journal.
    Locked,
}

impl fmt::Display for JournalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "the journal could not be read or written: {e}"),
            Self::Format(found) => write!(
                f,
                "the journal is format {found}; this version reads {FORMAT}"
            ),
            Self::Record(e) => write!(f, "a record could not be written: {e}"),
            Self::Locked => f.write_str("another terminal is keeping this account's journal"),
        }
    }
}

impl std::error::Error for JournalError {}

impl From<io::Error> for JournalError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct Header {
    journal: u64,
    date: String,
}

/// The right to keep an account's journal, held while the terminal runs, so
/// two terminals on one account cannot write over each other. Released when
/// dropped, or by the system when the process ends.
pub struct AccountLock {
    _file: File,
}

impl AccountLock {
    /// Takes the lock on `folder`, creating it if need be.
    pub fn acquire(folder: &Path) -> Result<Self, JournalError> {
        fs::create_dir_all(folder)?;
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(folder.join(LOCK_FILE))?;
        match file.try_lock() {
            Ok(()) => Ok(Self { _file: file }),
            Err(TryLockError::WouldBlock) => Err(JournalError::Locked),
            Err(TryLockError::Error(e)) => Err(e.into()),
        }
    }
}

/// What of `restored` must outlive its day: orders still open, conditional
/// orders still pending or waiting, the trader's switch, and an id
/// high-water mark past every order it holds, finished ones included.
pub fn live(restored: &Restored) -> Restored {
    let past_every = restored
        .orders
        .iter()
        .map(|record| record.order.id.0.saturating_add(1))
        .chain(restored.next_order_id)
        .max();
    Restored {
        orders: restored
            .orders
            .iter()
            .filter(|record| record.order.status.is_open())
            .cloned()
            .collect(),
        conditionals: restored
            .conditionals
            .iter()
            .filter(|c| {
                matches!(
                    c.status,
                    ConditionalStatus::Pending | ConditionalStatus::Waiting
                )
            })
            .cloned()
            .collect(),
        trading: restored.trading,
        next_order_id: past_every,
    }
}

/// The records that rebuild `restored` exactly.
pub fn records_of(restored: &Restored) -> Result<Vec<Record>, RecordError> {
    let mut records = Vec::new();
    for order in &restored.orders {
        records.push(Record::order(order)?);
    }
    for conditional in &restored.conditionals {
        records.push(Record::conditional(conditional)?);
    }
    records.extend(restored.trading.map(Record::trading));
    records.extend(restored.next_order_id.map(Record::reserved));
    Ok(records)
}

/// The folder an account's journals live in: `base`, then a hash of the
/// venue and the account, so the account's identifier is never on disk.
pub fn account_folder(base: &Path, venue: &str, account: &str) -> PathBuf {
    base.join(format!(
        "{:016x}",
        fnv1a(format!("{venue}\0{account}").as_bytes())
    ))
}

/// FNV-1a: stable across Rust versions and platforms, unlike std's hasher.
fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

/// The UTC date `unix_seconds` falls on, as `YYYY-MM-DD`.
pub fn utc_date(unix_seconds: u64) -> String {
    // Howard Hinnant's civil-from-days.
    let days = i64::try_from(unix_seconds / 86_400).unwrap_or(i64::MAX) + 719_468;
    let era = days.div_euclid(146_097);
    let day_of_era = days.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

/// A day's journal, open for appending.
pub struct Journal {
    file: File,
    path: PathBuf,
    /// A write was cut short and could not be cut back off: the next one
    /// starts on a fresh line, so it is not glued to the torn one.
    torn: bool,
}

/// What reading a day's file found.
struct Read {
    restored: Restored,
    damaged: Option<PathBuf>,
    /// Readable records beyond the header.
    records: usize,
}

/// A day's journal as opened: the session it holds, and where its damaged
/// tail went, if it had one.
pub struct Opened {
    pub journal: Journal,
    pub restored: Restored,
    pub damaged: Option<PathBuf>,
}

impl Journal {
    /// Opens `date`'s journal in `folder`, creating both if need be, and
    /// rebuilds the session it records. A day whose file holds no records
    /// yet starts from what was live in the latest earlier one, written into
    /// the new file whole or not at all. Take the folder's [`AccountLock`]
    /// first.
    pub fn open(folder: &Path, date: &str) -> Result<Opened, JournalError> {
        fs::create_dir_all(folder)?;
        let path = folder.join(format!("{date}.{EXTENSION}"));
        let today = match fs::read(&path) {
            Ok(bytes) => Some(read(&path, &bytes, date)?),
            Err(e) if e.kind() == io::ErrorKind::NotFound => None,
            Err(e) => return Err(e.into()),
        };
        let (restored, damaged) = match today {
            Some(found) if found.records > 0 => (found.restored, found.damaged),
            found => {
                let mut damaged = found.and_then(|found| found.damaged);
                let carried = match latest_before(folder, date)? {
                    Some((day, earlier)) => {
                        let bytes = fs::read(&earlier)?;
                        let before = read(&earlier, &bytes, &day)?;
                        damaged = damaged.or(before.damaged);
                        live(&before.restored)
                    }
                    None => Restored::default(),
                };
                let mut text = header_line(date)?;
                for record in records_of(&carried).map_err(JournalError::Record)? {
                    text.push_str(&line_of(&record)?);
                }
                replace(&path, text.as_bytes())?;
                (carried, damaged)
            }
        };
        let mut file = OpenOptions::new().write(true).open(&path)?;
        file.seek(SeekFrom::End(0))?;
        Ok(Opened {
            journal: Self {
                file,
                path,
                torn: false,
            },
            restored,
            damaged,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Appends `records`, one line each, and syncs them to the disk. A
    /// write that fails part way is cut back off; if even that fails, the
    /// next write starts on a fresh line.
    pub fn append(&mut self, records: &[Record]) -> Result<(), JournalError> {
        if records.is_empty() {
            return Ok(());
        }
        let mut text = String::new();
        if self.torn {
            text.push('\n');
        }
        for record in records {
            text.push_str(&line_of(record)?);
        }
        let before = self.file.seek(SeekFrom::End(0))?;
        let written = self
            .file
            .write_all(text.as_bytes())
            .and_then(|()| self.file.sync_data());
        match written {
            Ok(()) => {
                self.torn = false;
                Ok(())
            }
            Err(e) => {
                self.torn = self.cut_back(before).is_err();
                Err(e.into())
            }
        }
    }

    /// Cuts the file back to `length`, where the next write will go.
    fn cut_back(&mut self, length: u64) -> io::Result<()> {
        self.file.set_len(length)?;
        self.file.seek(SeekFrom::End(0)).map(drop)
    }
}

fn line_of(record: &Record) -> Result<String, JournalError> {
    let line = serde_json::to_string(record).map_err(io::Error::other)?;
    Ok(format!("{line}\n"))
}

fn header_line(date: &str) -> Result<String, JournalError> {
    let header = Header {
        journal: FORMAT,
        date: date.to_string(),
    };
    let line = serde_json::to_string(&header).map_err(io::Error::other)?;
    Ok(format!("{line}\n"))
}

/// The latest day's journal in `folder` dated before `date`, and its date.
fn latest_before(folder: &Path, date: &str) -> Result<Option<(String, PathBuf)>, JournalError> {
    let mut latest: Option<(String, PathBuf)> = None;
    for entry in fs::read_dir(folder)? {
        let path = entry?.path();
        let is_journal = path.extension().is_some_and(|ext| ext == EXTENSION);
        let Some(day) = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .map(str::to_string)
        else {
            continue;
        };
        let dated = day.len() == 10 && day.chars().all(|c| c.is_ascii_digit() || c == '-');
        if is_journal
            && dated
            && day.as_str() < date
            && latest.as_ref().is_none_or(|(best, _)| day > *best)
        {
            latest = Some((day, path));
        }
    }
    Ok(latest)
}

/// The session `bytes` records, and where lines that could not be read were
/// moved. Such a line is skipped, not the end of reading: each line is the
/// whole state of one thing, so the rest still holds. A last line the
/// process did not finish counts as one; a header that names no format is
/// replaced with `date`'s. The journal is then rewritten, atomically, with
/// its readable lines only.
fn read(path: &Path, bytes: &[u8], date: &str) -> Result<Read, JournalError> {
    let mut orders: BTreeMap<u64, OrderRecord> = BTreeMap::new();
    let mut conditionals: BTreeMap<u64, Conditional> = BTreeMap::new();
    let mut trading: Option<TradingState> = None;
    let mut reserved: Option<u64> = None;
    let fresh_header = header_line(date)?;
    let mut good: Vec<&[u8]> = Vec::new();
    let mut bad: Vec<&[u8]> = Vec::new();
    let mut records = 0;
    for (index, line) in bytes.split_inclusive(|b| *b == b'\n').enumerate() {
        let Some(body) = line.strip_suffix(b"\n") else {
            bad.push(line);
            continue;
        };
        if index == 0 {
            match header_format(body) {
                Some(FORMAT) => good.push(line),
                Some(other) => return Err(JournalError::Format(other)),
                None => {
                    bad.push(line);
                    good.push(fresh_header.as_bytes());
                }
            }
            continue;
        }
        // The fresh line a write starts on after one was cut short.
        if body.is_empty() {
            continue;
        }
        let applied = serde_json::from_slice::<Record>(body)
            .map_err(|e| RecordError(e.to_string()))
            .and_then(|record| {
                fold(
                    record,
                    &mut orders,
                    &mut conditionals,
                    &mut trading,
                    &mut reserved,
                )
            });
        match applied {
            Ok(()) => {
                good.push(line);
                records += 1;
            }
            Err(_) => bad.push(line),
        }
    }
    let damaged = if bad.is_empty() {
        None
    } else {
        let aside = path.with_extension(format!("{EXTENSION}.damaged"));
        let mut file = OpenOptions::new().create(true).append(true).open(&aside)?;
        for line in &bad {
            file.write_all(line)?;
            if !line.ends_with(b"\n") {
                file.write_all(b"\n")?;
            }
        }
        file.sync_data()?;
        rewrite(path, &good)?;
        Some(aside)
    };
    let restored = Restored {
        orders: orders.into_values().collect(),
        conditionals: conditionals.into_values().collect(),
        trading,
        next_order_id: reserved,
    };
    Ok(Read {
        restored,
        damaged,
        records,
    })
}

/// The format a header line names, or `None` when it names none.
fn header_format(body: &[u8]) -> Option<u64> {
    serde_json::from_slice::<serde_json::Value>(body)
        .ok()?
        .get("journal")?
        .as_u64()
}

/// Replaces `path` with `lines`, through a temporary file and a rename, so
/// a crash leaves either the old journal or the new one whole.
fn rewrite(path: &Path, lines: &[&[u8]]) -> Result<(), JournalError> {
    replace(path, &lines.concat())
}

/// Replaces `path` with `bytes` whole: written and synced under another
/// name, then renamed over it.
fn replace(path: &Path, bytes: &[u8]) -> Result<(), JournalError> {
    let temporary = path.with_extension(format!("{EXTENSION}.rewrite"));
    let mut file = File::create(&temporary)?;
    file.write_all(bytes)?;
    file.sync_data()?;
    drop(file);
    fs::rename(&temporary, path)?;
    Ok(())
}

fn fold(
    record: Record,
    orders: &mut BTreeMap<u64, OrderRecord>,
    conditionals: &mut BTreeMap<u64, Conditional>,
    trading: &mut Option<TradingState>,
    reserved: &mut Option<u64>,
) -> Result<(), RecordError> {
    match record {
        Record::Order(line) => {
            let order = line.to_record()?;
            orders.insert(order.order.id.0, order);
        }
        Record::Conditional(line) => {
            let conditional = line.to_conditional()?;
            conditionals.insert(conditional.id.0, conditional);
        }
        Record::Trading(raw) => *trading = Some(record::parse_trading(&raw)?),
        Record::Reserved(next) => *reserved = Some(reserved.map_or(next, |was| was.max(next))),
    }
    Ok(())
}

#[cfg(test)]
mod tests;
