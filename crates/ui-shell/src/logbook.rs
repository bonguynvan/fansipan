//! The terminal's log: what the shell and engines report, one
//! line each, in a file per UTC day that a trader can attach to a problem
//! report. It says what happened to which order and connection, and why.
//! It writes no price, quantity, or account figure of its own; a reason or
//! an error is kept as written, since that is what a report needs, and
//! some name the figure they are about. Folders it is told to hide, such
//! as the trader's home and the account's journal, appear by a stand-in.
//!
//! Files live in one folder, `fansipan-YYYY-MM-DD.log`; the last
//! [`KEEP_DAYS`] are kept and older ones deleted when a day's file opens. A
//! day's file stops at [`MAX_DAY_BYTES`], with a line saying so, so a
//! runaway report cannot fill a disk. Everything also goes to standard
//! error, so a terminal started from a console still shows it.

use std::collections::HashMap;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

/// How many days of files are kept, today's included.
pub const KEEP_DAYS: usize = 7;
/// Where a day's file stops.
pub const MAX_DAY_BYTES: u64 = 5 * 1024 * 1024;
/// Where one message is cut, so one report cannot crowd out the rest.
const MAX_MESSAGE_CHARS: usize = 2_000;
const PREFIX: &str = "fansipan-";
const SUFFIX: &str = ".log";
const SECONDS_PER_DAY: u64 = 86_400;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Info,
    Warn,
    Error,
}

impl fmt::Display for Level {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Info => "INFO ",
            Self::Warn => "WARN ",
            Self::Error => "ERROR",
        })
    }
}

/// What was last logged of an order: its status, its reason, and whether
/// the venue had not answered.
type OrderMark = (&'static str, Option<String>, bool);

/// One folder of daily files, and today's open.
pub struct Logbook {
    folder: PathBuf,
    day: Option<Day>,
    /// A date whose file could not be opened: not tried again that day.
    failed: Option<String>,
    /// Text replaced before a line is written: a folder, and its stand-in.
    hidden: Vec<(String, String)>,
    orders: HashMap<u64, OrderMark>,
}

struct Day {
    date: String,
    file: File,
    written: u64,
    full: bool,
}

impl Logbook {
    pub fn new(folder: PathBuf) -> Self {
        Self {
            folder,
            day: None,
            failed: None,
            hidden: Vec::new(),
            orders: HashMap::new(),
        }
    }

    pub fn folder(&self) -> &Path {
        &self.folder
    }

    /// Writes `stand_in` wherever `path` would appear in a line. The longest
    /// path hidden is replaced first, so a folder inside another hidden one
    /// keeps its own stand-in.
    pub fn hide(&mut self, path: &Path, stand_in: &str) {
        let text = path.display().to_string();
        if text.is_empty() {
            return;
        }
        self.hidden.push((text, stand_in.to_string()));
        self.hidden
            .sort_by_key(|(text, _)| std::cmp::Reverse(text.len()));
    }

    /// Writes one line at `unix_millis`. A file that cannot be opened or
    /// written is given up for the day, with the reason on standard error:
    /// the log must never stop the terminal.
    pub fn write(&mut self, unix_millis: u64, level: Level, message: &str) {
        let date = journal::utc_date(unix_millis / 1000);
        if self.day.as_ref().is_none_or(|day| day.date != date) {
            if self.failed.as_deref() == Some(date.as_str()) {
                return;
            }
            self.day = match self.open_day(&date) {
                Ok(day) => Some(day),
                Err(e) => {
                    to_stderr(&format!("log: cannot open today's file: {e}"));
                    self.failed = Some(date);
                    None
                }
            };
        }
        let message = self
            .hidden
            .iter()
            .fold(message.to_string(), |text, (path, stand_in)| {
                text.replace(path.as_str(), stand_in)
            });
        let Some(day) = self.day.as_mut() else {
            return;
        };
        if day.full {
            return;
        }
        let mut line = format_line(unix_millis, level, &message);
        if day.written + line.len() as u64 > MAX_DAY_BYTES {
            line = format_line(
                unix_millis,
                Level::Warn,
                "today's log is full; later lines are left out until tomorrow",
            );
            day.full = true;
        }
        match day.file.write_all(line.as_bytes()) {
            Ok(()) => day.written += line.len() as u64,
            Err(e) => {
                to_stderr(&format!("log: cannot write: {e}"));
                day.full = true;
            }
        }
    }

    /// Logs an order when its status, its reason, or whether the venue has
    /// answered differs from what was last logged of it: which order, what
    /// it is, and why, never its price or quantity.
    pub fn order(
        &mut self,
        unix_millis: u64,
        id: u64,
        what: &str,
        status: &'static str,
        reason: Option<&str>,
        unanswered: bool,
    ) {
        let mark = (status, reason.map(str::to_string), unanswered);
        if self.orders.get(&id) == Some(&mark) {
            return;
        }
        self.orders.insert(id, mark);
        let level = match status {
            "denied" | "rejected" => Level::Warn,
            _ if unanswered => Level::Warn,
            _ => Level::Info,
        };
        let waiting = if unanswered {
            ", no answer from the venue"
        } else {
            ""
        };
        let message = match reason {
            Some(reason) => format!("order {id} {what}: {status}{waiting} ({reason})"),
            None => format!("order {id} {what}: {status}{waiting}"),
        };
        self.write(unix_millis, level, &message);
    }

    fn open_day(&self, date: &str) -> io::Result<Day> {
        fs::create_dir_all(&self.folder)?;
        let name = format!("{PREFIX}{date}{SUFFIX}");
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.folder.join(&name))?;
        let written = file.metadata()?.len();
        prune(&self.folder, &name, KEEP_DAYS);
        Ok(Day {
            date: date.to_string(),
            file,
            written,
            full: written >= MAX_DAY_BYTES,
        })
    }
}

/// One line: the UTC time to the millisecond, the level, and the message on
/// one line, cut at [`MAX_MESSAGE_CHARS`].
fn format_line(unix_millis: u64, level: Level, message: &str) -> String {
    let seconds = unix_millis / 1000;
    let of_day = seconds % SECONDS_PER_DAY;
    let one_line: String = message
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(MAX_MESSAGE_CHARS)
        .collect();
    format!(
        "{}T{:02}:{:02}:{:02}.{:03}Z {level} {one_line}\n",
        journal::utc_date(seconds),
        of_day / 3600,
        of_day / 60 % 60,
        of_day % 60,
        unix_millis % 1000,
    )
}

/// Deletes all but the newest `keep` daily files in `folder`, never
/// `today`, the one just opened. A file dated after `today` means the clock
/// is wrong, now or when that file was written, so nothing is deleted:
/// counting from a wrong date could take today's file or real history.
/// Files of any other name are left alone. The names sort by date.
fn prune(folder: &Path, today: &str, keep: usize) {
    let Ok(entries) = fs::read_dir(folder) else {
        return;
    };
    let mut days: Vec<String> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| name.starts_with(PREFIX) && name.ends_with(SUFFIX))
        .collect();
    days.sort();
    if days.last().is_some_and(|newest| newest.as_str() > today) {
        return;
    }
    let old = days.len().saturating_sub(keep);
    for name in days[..old].iter().filter(|name| name.as_str() != today) {
        if let Err(e) = fs::remove_file(folder.join(name)) {
            to_stderr(&format!("log: cannot delete {name}: {e}"));
        }
    }
}

/// Standard error, ignoring a stream that cannot be written: `eprintln!`
/// would panic, and a log line is not worth a thread.
fn to_stderr(line: &str) {
    let _ = writeln!(io::stderr(), "{line}");
}

static BOOK: OnceLock<Mutex<Logbook>> = OnceLock::new();

/// Starts writing to `folder`. Until then, and if it is never called (as in
/// tests), lines go to standard error only.
pub fn start(folder: PathBuf) {
    if BOOK.set(Mutex::new(Logbook::new(folder))).is_err() {
        to_stderr("log: already started");
    }
}

/// See [`Logbook::hide`].
pub fn hide(path: &Path, stand_in: &str) {
    with_book(|book, _| book.hide(path, stand_in));
}

/// The folder the log is written to, once started.
pub fn folder() -> Option<PathBuf> {
    BOOK.get().map(|book| {
        book.lock()
            .unwrap_or_else(PoisonError::into_inner)
            .folder()
            .to_path_buf()
    })
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| {
            u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
        })
}

fn with_book(act: impl FnOnce(&mut Logbook, u64)) {
    if let Some(book) = BOOK.get() {
        act(
            &mut book.lock().unwrap_or_else(PoisonError::into_inner),
            now_millis(),
        );
    }
}

pub fn log(level: Level, message: &str) {
    to_stderr(&format!("{level} {message}"));
    with_book(|book, now| book.write(now, level, message));
}

pub fn info(message: &str) {
    log(Level::Info, message);
}

pub fn warn(message: &str) {
    log(Level::Warn, message);
}

pub fn error(message: &str) {
    log(Level::Error, message);
}

/// See [`Logbook::order`].
pub fn order(id: u64, what: &str, status: &'static str, reason: Option<&str>, unanswered: bool) {
    with_book(|book, now| book.order(now, id, what, status, reason, unanswered));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    /// 2026-10-19T09:30:05.042Z.
    const MORNING: u64 = 1_792_402_205_042;
    const DAY_MILLIS: u64 = SECONDS_PER_DAY * 1000;

    /// A fresh folder of its own for each test.
    fn folder() -> PathBuf {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "fansipan-logbook-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&path);
        path
    }

    fn read(folder: &Path, date: &str) -> String {
        fs::read_to_string(folder.join(format!("{PREFIX}{date}{SUFFIX}"))).unwrap_or_default()
    }

    #[test]
    fn a_line_has_the_utc_time_the_level_and_the_message_on_one_line() {
        let line = format_line(MORNING, Level::Warn, "first\nsecond\tthird");

        assert_eq!(line, "2026-10-19T09:30:05.042Z WARN  first second third\n");
    }

    #[test]
    fn a_long_message_is_cut_where_it_should_be() {
        let line = format_line(MORNING, Level::Info, &"x".repeat(MAX_MESSAGE_CHARS * 2));

        assert_eq!(line.matches('x').count(), MAX_MESSAGE_CHARS);
    }

    #[test]
    fn each_day_gets_its_own_file_and_only_the_last_week_is_kept() {
        let folder = folder();
        let mut book = Logbook::new(folder.clone());

        for day in 0..10 {
            book.write(
                MORNING + day * DAY_MILLIS,
                Level::Info,
                &format!("day {day}"),
            );
        }

        let mut names: Vec<String> = fs::read_dir(&folder)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        assert_eq!(names.len(), KEEP_DAYS);
        assert_eq!(names[0], "fansipan-2026-10-22.log");
        assert!(read(&folder, "2026-10-28").contains("day 9"));
        let _ = fs::remove_dir_all(&folder);
    }

    #[test]
    fn a_day_continues_its_file_after_a_restart() {
        let folder = folder();
        Logbook::new(folder.clone()).write(MORNING, Level::Info, "before");

        Logbook::new(folder.clone()).write(MORNING + 1000, Level::Info, "after");

        let today = read(&folder, "2026-10-19");
        assert!(
            today.contains("before") && today.contains("after"),
            "{today}"
        );
        let _ = fs::remove_dir_all(&folder);
    }

    #[test]
    fn a_full_day_says_so_once_and_takes_nothing_more() {
        let folder = folder();
        let mut book = Logbook::new(folder.clone());
        let big = "y".repeat(MAX_MESSAGE_CHARS);

        let lines = MAX_DAY_BYTES as usize / MAX_MESSAGE_CHARS + 10;
        for _ in 0..lines {
            book.write(MORNING, Level::Info, &big);
        }
        book.write(MORNING, Level::Error, "after the cap");

        let today = read(&folder, "2026-10-19");
        assert!(today.len() as u64 <= MAX_DAY_BYTES + 200);
        assert_eq!(today.matches("log is full").count(), 1);
        assert!(!today.contains("after the cap"));
        book.write(MORNING + DAY_MILLIS, Level::Info, "next day");
        assert!(read(&folder, "2026-10-20").contains("next day"));
        let _ = fs::remove_dir_all(&folder);
    }

    #[test]
    fn an_order_is_logged_when_its_status_moves_and_not_on_every_update() {
        let folder = folder();
        let mut book = Logbook::new(folder.clone());

        book.order(MORNING, 7, "AAPL buy", "submitted", None, false);
        book.order(MORNING, 7, "AAPL buy", "submitted", None, false);
        book.order(MORNING, 7, "AAPL buy", "submitted", None, true);
        book.order(MORNING, 7, "AAPL buy", "rejected", Some("no funds"), false);

        let today = read(&folder, "2026-10-19");
        assert_eq!(
            today
                .matches(
                    "order 7 AAPL buy: submitted
"
                )
                .count(),
            1
        );
        assert!(today.contains("WARN  order 7 AAPL buy: submitted, no answer from the venue"));
        assert!(today.contains("WARN  order 7 AAPL buy: rejected (no funds)"));
        let _ = fs::remove_dir_all(&folder);
    }

    #[test]
    fn files_not_the_logs_own_are_left_alone() {
        let folder = folder();
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join("notes.txt"), "mine").unwrap();
        let mut book = Logbook::new(folder.clone());

        for day in 0..10 {
            book.write(MORNING + day * DAY_MILLIS, Level::Info, "x");
        }

        assert!(folder.join("notes.txt").exists());
        let _ = fs::remove_dir_all(&folder);
    }

    #[test]
    fn a_new_reason_on_the_same_status_is_logged() {
        let folder = folder();
        let mut book = Logbook::new(folder.clone());

        book.order(MORNING, 8, "AAPL sell", "partially_filled", None, false);
        book.order(
            MORNING,
            8,
            "AAPL sell",
            "partially_filled",
            Some("fills with no price"),
            false,
        );

        assert!(read(&folder, "2026-10-19").contains("(fills with no price)"));
        let _ = fs::remove_dir_all(&folder);
    }

    #[test]
    fn a_wrong_clock_deletes_nothing_and_today_is_never_deleted() {
        let folder = folder();
        let mut book = Logbook::new(folder.clone());
        for day in 0..KEEP_DAYS as u64 {
            book.write(MORNING + day * DAY_MILLIS, Level::Info, "real");
        }

        // A clock set back a month, then one at the epoch.
        let mut wrong = Logbook::new(folder.clone());
        wrong.write(MORNING - 30 * DAY_MILLIS, Level::Info, "a month back");
        wrong.write(0, Level::Info, "the epoch");

        let count = fs::read_dir(&folder).unwrap().count();
        assert_eq!(count, KEEP_DAYS + 2, "nothing deleted");
        assert!(read(&folder, "2026-09-19").contains("a month back"));
        let _ = fs::remove_dir_all(&folder);
    }

    #[test]
    fn a_folder_that_cannot_be_made_is_given_up_for_the_day() {
        let blocker = folder();
        fs::create_dir_all(blocker.parent().unwrap()).unwrap();
        fs::write(&blocker, "a file where the folder should be").unwrap();
        let mut book = Logbook::new(blocker.join("logs"));

        book.write(MORNING, Level::Info, "lost");

        assert_eq!(book.failed.as_deref(), Some("2026-10-19"));
        book.write(MORNING + 1000, Level::Info, "lost too");
        assert!(book.day.is_none());
        let _ = fs::remove_file(&blocker);
    }

    #[test]
    fn hidden_folders_appear_by_their_stand_ins() {
        let folder = folder();
        let mut book = Logbook::new(folder.clone());
        let home = std::env::temp_dir().join("someone");
        let account = home.join("data").join("journal").join("abc123");
        book.hide(&home, "~");
        book.hide(&account, "<account journal>");

        book.write(
            MORNING,
            Level::Error,
            &format!(
                "journal: cannot write {}; settings in {}",
                account.join("2026-10-19.jsonl").display(),
                home.join("x").display()
            ),
        );

        let today = read(&folder, "2026-10-19");
        assert!(today.contains("cannot write <account journal>"), "{today}");
        assert!(today.contains("settings in ~"), "{today}");
        assert!(
            !today.contains("someone") && !today.contains("abc123"),
            "{today}"
        );
        let _ = fs::remove_dir_all(&folder);
    }
}
