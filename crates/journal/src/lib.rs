//! The engine's records on disk, and the session rebuilt from them
//! (ADR 0023). One folder per account, named by a hash of it, and one file
//! per UTC date: a header line, then one record per line, each the whole
//! state of one thing, the last for each thing winning. A line that does
//! not parse, and everything after it, is moved to a `.damaged` file beside
//! the journal rather than half-applied.

mod record;

use std::collections::BTreeMap;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use execution_engine::{Conditional, OrderRecord, Restored, TradingState};
use serde::{Deserialize, Serialize};

pub use record::{ConditionalLine, OrderLine, Record, RecordError};

/// The journal format this version writes and reads.
pub const FORMAT: u64 = 1;
const EXTENSION: &str = "jsonl";

#[derive(Debug)]
pub enum JournalError {
    Io(io::Error),
    /// A journal written by a newer version; it is left as it is.
    Format(u64),
    Record(RecordError),
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
    /// rebuilds the session it records.
    pub fn open(folder: &Path, date: &str) -> Result<Opened, JournalError> {
        fs::create_dir_all(folder)?;
        let path = folder.join(format!("{date}.{EXTENSION}"));
        let text = match fs::read(&path) {
            Ok(bytes) => Some(bytes),
            Err(e) if e.kind() == io::ErrorKind::NotFound => None,
            Err(e) => return Err(e.into()),
        };
        let (restored, damaged) = match &text {
            Some(bytes) => read(&path, bytes)?,
            None => (Restored::default(), None),
        };
        let mut file = OpenOptions::new().create(true).append(true).open(&path)?;
        if file.metadata()?.len() == 0 {
            let header = Header {
                journal: FORMAT,
                date: date.to_string(),
            };
            let line = serde_json::to_string(&header).map_err(io::Error::other)?;
            file.write_all(format!("{line}\n").as_bytes())?;
            file.sync_data()?;
        }
        Ok(Opened {
            journal: Self { file, path },
            restored,
            damaged,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Appends `records`, one line each, and syncs them to the disk.
    pub fn append(&mut self, records: &[Record]) -> Result<(), JournalError> {
        if records.is_empty() {
            return Ok(());
        }
        let mut text = String::new();
        for record in records {
            text.push_str(&serde_json::to_string(record).map_err(io::Error::other)?);
            text.push('\n');
        }
        self.file.write_all(text.as_bytes())?;
        self.file.sync_data()?;
        Ok(())
    }
}

/// The session `bytes` records, and where a damaged tail was moved. The
/// journal file is cut back to its last good line.
fn read(path: &Path, bytes: &[u8]) -> Result<(Restored, Option<PathBuf>), JournalError> {
    let mut orders: BTreeMap<u64, OrderRecord> = BTreeMap::new();
    let mut conditionals: BTreeMap<u64, Conditional> = BTreeMap::new();
    let mut trading: Option<TradingState> = None;
    let mut good = 0;
    let mut first = true;
    for line in bytes.split_inclusive(|b| *b == b'\n') {
        // A line with no end is a write the process did not finish.
        let Some(body) = line.strip_suffix(b"\n") else {
            break;
        };
        if first {
            first = false;
            match serde_json::from_slice::<Header>(body) {
                Ok(header) if header.journal == FORMAT => {}
                Ok(header) => return Err(JournalError::Format(header.journal)),
                Err(_) => break,
            }
        } else {
            let applied = serde_json::from_slice::<Record>(body)
                .map_err(|e| RecordError(e.to_string()))
                .and_then(|record| fold(record, &mut orders, &mut conditionals, &mut trading));
            if applied.is_err() {
                break;
            }
        }
        good += line.len();
    }
    let damaged = if good < bytes.len() {
        let aside = path.with_extension(format!("{EXTENSION}.damaged"));
        let mut file = OpenOptions::new().create(true).append(true).open(&aside)?;
        file.write_all(&bytes[good..])?;
        file.sync_data()?;
        OpenOptions::new()
            .write(true)
            .open(path)?
            .set_len(good as u64)?;
        Some(aside)
    } else {
        None
    };
    let restored = Restored {
        orders: orders.into_values().collect(),
        conditionals: conditionals.into_values().collect(),
        trading,
    };
    Ok((restored, damaged))
}

fn fold(
    record: Record,
    orders: &mut BTreeMap<u64, OrderRecord>,
    conditionals: &mut BTreeMap<u64, Conditional>,
    trading: &mut Option<TradingState>,
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
    }
    Ok(())
}

#[cfg(test)]
mod tests;
