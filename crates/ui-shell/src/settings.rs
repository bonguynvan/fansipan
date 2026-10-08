//! A small persistent key-value store for the front end: layouts, window
//! lists, link-group selections, and any screen's own settings. Keys are
//! namespaced by convention (`workspace.layout.main`, `screen.chart.style`);
//! values are arbitrary JSON the shell never interprets.
//!
//! One JSON file, rewritten on every change: written to a temporary file,
//! flushed to disk, then renamed over the old one, so a crash or power loss
//! mid-write leaves the previous version intact.

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

use serde_json::Value;

/// Longest key accepted.
pub const MAX_KEY_LEN: usize = 128;

/// What one store may hold. A full multi-panel layout is a few kilobytes;
/// the caps leave room for screens' own settings while keeping a buggy
/// screen from filling the disk or making every save slow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub max_value_bytes: usize,
    pub max_keys: usize,
    pub max_total_bytes: usize,
}

impl Limits {
    pub const DEFAULT: Self = Self {
        max_value_bytes: 256 * 1024,
        max_keys: 1024,
        max_total_bytes: 8 * 1024 * 1024,
    };
}

#[derive(Debug, Clone, PartialEq)]
pub enum SettingsError {
    InvalidKey(String),
    ValueTooLarge {
        key: String,
        bytes: usize,
        limit: usize,
    },
    TooManyKeys {
        limit: usize,
    },
    StoreFull {
        bytes: usize,
        limit: usize,
    },
    /// The file existed but could not be read at start; it is left alone
    /// rather than overwritten with this session's (empty) view of it.
    ReadOnly,
    Io(String),
}

impl fmt::Display for SettingsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidKey(key) => write!(
                f,
                "invalid settings key \"{key}\": use 1-{MAX_KEY_LEN} of a-z, 0-9, '.', '-', '_', starting with a letter or digit"
            ),
            Self::ValueTooLarge { key, bytes, limit } => {
                write!(f, "value for \"{key}\" is {bytes} bytes; the limit is {limit}")
            }
            Self::TooManyKeys { limit } => write!(f, "settings hold at most {limit} keys"),
            Self::StoreFull { bytes, limit } => {
                write!(f, "settings would be {bytes} bytes; the limit is {limit}")
            }
            Self::ReadOnly => f.write_str(
                "the settings file could not be read at start, so it is not being overwritten",
            ),
            Self::Io(message) => write!(f, "settings could not be saved: {message}"),
        }
    }
}

impl std::error::Error for SettingsError {}

pub struct Settings {
    path: PathBuf,
    values: BTreeMap<String, Value>,
    limits: Limits,
    writable: bool,
}

impl Settings {
    pub fn load(path: impl Into<PathBuf>) -> (Self, Option<String>) {
        Self::load_with(path, Limits::DEFAULT)
    }

    /// Loads the store at `path`, returning a warning to show if something
    /// was wrong with it:
    /// - missing: an empty store, no warning;
    /// - present but not a JSON object (bad JSON, or not text at all, as a
    ///   torn write can leave it): moved aside as `<path>.bad`, and the store
    ///   starts empty, so one corrupt write cannot stop the terminal; if it
    ///   cannot be moved, the store starts empty and refuses writes, so the
    ///   only copy is never overwritten;
    /// - present but unreadable (permissions, a lock): the store starts empty
    ///   and refuses writes, so the real file is never overwritten.
    pub fn load_with(path: impl Into<PathBuf>, limits: Limits) -> (Self, Option<String>) {
        let path = path.into();
        let empty = |writable| Self {
            path: path.clone(),
            values: BTreeMap::new(),
            limits,
            writable,
        };
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == ErrorKind::NotFound => return (empty(true), None),
            Err(e) => {
                let warning = format!(
                    "settings file {} could not be read ({e}); changes this session will not be saved",
                    path.display()
                );
                return (empty(false), Some(warning));
            }
        };
        match serde_json::from_slice::<BTreeMap<String, Value>>(&bytes) {
            Ok(values) => (
                Self {
                    path,
                    values,
                    limits,
                    writable: true,
                },
                None,
            ),
            Err(e) => {
                let aside = path.with_extension("json.bad");
                if fs::rename(&path, &aside).is_ok() {
                    let warning = format!(
                        "settings file unreadable ({e}); moved to {}; starting fresh",
                        aside.display()
                    );
                    return (empty(true), Some(warning));
                }
                let warning = format!(
                    "settings file unreadable ({e}) and could not be moved aside; \
                     changes this session will not be saved"
                );
                (empty(false), Some(warning))
            }
        }
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        self.values.get(key)
    }

    /// Stores `value` and saves. On error nothing changes, in memory or on disk.
    pub fn set(&mut self, key: &str, value: Value) -> Result<(), SettingsError> {
        validate_key(key)?;
        let bytes = serde_json::to_vec(&value)
            .map_err(|e| SettingsError::Io(e.to_string()))?
            .len();
        if bytes > self.limits.max_value_bytes {
            return Err(SettingsError::ValueTooLarge {
                key: key.to_string(),
                bytes,
                limit: self.limits.max_value_bytes,
            });
        }
        if !self.values.contains_key(key) && self.values.len() >= self.limits.max_keys {
            return Err(SettingsError::TooManyKeys {
                limit: self.limits.max_keys,
            });
        }
        let previous = self.values.insert(key.to_string(), value);
        self.save().inspect_err(|_| {
            match previous {
                Some(old) => self.values.insert(key.to_string(), old),
                None => self.values.remove(key),
            };
        })
    }

    /// Removing a key that is not there is not an error.
    pub fn remove(&mut self, key: &str) -> Result<(), SettingsError> {
        validate_key(key)?;
        let Some(previous) = self.values.remove(key) else {
            return Ok(());
        };
        self.save().inspect_err(|_| {
            self.values.insert(key.to_string(), previous);
        })
    }

    fn save(&self) -> Result<(), SettingsError> {
        if !self.writable {
            return Err(SettingsError::ReadOnly);
        }
        let text = serde_json::to_string_pretty(&self.values)
            .map_err(|e| SettingsError::Io(e.to_string()))?;
        if text.len() > self.limits.max_total_bytes {
            return Err(SettingsError::StoreFull {
                bytes: text.len(),
                limit: self.limits.max_total_bytes,
            });
        }
        write_atomically(&self.path, text.as_bytes()).map_err(|e| SettingsError::Io(e.to_string()))
    }
}

/// Temporary file, flushed to disk, then renamed over `path`.
fn write_atomically(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let temporary = path.with_extension("json.tmp");
    let mut file = fs::File::create(&temporary)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    fs::rename(&temporary, path)
}

fn validate_key(key: &str) -> Result<(), SettingsError> {
    let allowed =
        |c: char| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '-' | '_');
    let valid = !key.is_empty()
        && key.len() <= MAX_KEY_LEN
        && key.chars().all(allowed)
        && key.starts_with(|c: char| c.is_ascii_lowercase() || c.is_ascii_digit());
    if valid {
        Ok(())
    } else {
        Err(SettingsError::InvalidKey(key.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// A fresh directory per test, removed when dropped.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir()
                .join(format!("fansipan-settings-{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        fn file(&self) -> PathBuf {
            self.0.join("settings.json")
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    const SMALL: Limits = Limits {
        max_value_bytes: 64,
        max_keys: 3,
        max_total_bytes: 200,
    };

    #[test]
    fn a_missing_file_is_an_empty_store() {
        let dir = TempDir::new("missing");

        let (settings, warning) = Settings::load(dir.file());

        assert!(settings.get("anything").is_none());
        assert!(warning.is_none());
    }

    #[test]
    fn values_survive_a_reload() {
        let dir = TempDir::new("reload");
        let (mut settings, _) = Settings::load(dir.file());

        settings
            .set("workspace.layout.main", json!({ "panels": 3 }))
            .unwrap();
        let (reloaded, _) = Settings::load(dir.file());

        assert_eq!(
            reloaded.get("workspace.layout.main"),
            Some(&json!({ "panels": 3 }))
        );
        assert!(!dir.0.join("settings.json.tmp").exists());
    }

    #[test]
    fn removing_a_key_persists_and_missing_keys_are_fine() {
        let dir = TempDir::new("remove");
        let (mut settings, _) = Settings::load(dir.file());
        settings.set("a", json!(1)).unwrap();

        settings.remove("a").unwrap();
        settings.remove("never-set").unwrap();

        assert!(Settings::load(dir.file()).0.get("a").is_none());
    }

    #[test]
    fn keys_are_validated() {
        let dir = TempDir::new("keys");
        let (mut settings, _) = Settings::load(dir.file());

        for bad in [
            "",
            "Upper",
            "../escape",
            "with space",
            ".leading",
            &"k".repeat(129),
        ] {
            assert!(
                matches!(
                    settings.set(bad, json!(1)),
                    Err(SettingsError::InvalidKey(_))
                ),
                "{bad:?}"
            );
        }
        assert!(settings.set("screen.chart_2.style-v1", json!(1)).is_ok());
    }

    #[test]
    fn oversized_values_are_refused_and_leave_the_store_unchanged() {
        let dir = TempDir::new("size");
        let (mut settings, _) = Settings::load_with(dir.file(), SMALL);
        settings.set("big", json!("small")).unwrap();

        let result = settings.set("big", json!("x".repeat(100)));

        assert!(matches!(result, Err(SettingsError::ValueTooLarge { .. })));
        assert_eq!(settings.get("big"), Some(&json!("small")));
    }

    #[test]
    fn the_number_of_keys_is_capped_but_existing_keys_can_change() {
        let dir = TempDir::new("keys-cap");
        let (mut settings, _) = Settings::load_with(dir.file(), SMALL);
        for key in ["a", "b", "c"] {
            settings.set(key, json!(1)).unwrap();
        }

        let fourth = settings.set("d", json!(1));
        let update = settings.set("a", json!(2));

        assert_eq!(fourth, Err(SettingsError::TooManyKeys { limit: 3 }));
        assert!(update.is_ok());
    }

    #[test]
    fn the_total_size_is_capped_and_a_refused_write_changes_nothing() {
        let dir = TempDir::new("total-cap");
        let (mut settings, _) = Settings::load_with(dir.file(), SMALL);
        settings.set("a", json!("x".repeat(60))).unwrap();
        settings.set("b", json!("x".repeat(60))).unwrap();

        let result = settings.set("c", json!("x".repeat(60)));

        assert!(matches!(result, Err(SettingsError::StoreFull { .. })));
        assert!(settings.get("c").is_none());
        assert!(Settings::load(dir.file()).0.get("c").is_none());
    }

    #[test]
    fn a_corrupt_file_is_set_aside_and_the_store_starts_empty() {
        let dir = TempDir::new("corrupt");
        fs::write(dir.file(), "{ not json").unwrap();

        let (mut settings, warning) = Settings::load(dir.file());

        assert!(settings.get("anything").is_none());
        assert!(warning.is_some_and(|w| w.contains("starting fresh")));
        assert!(dir.0.join("settings.json.bad").exists());
        assert!(settings.set("a", json!(1)).is_ok());
    }

    #[test]
    fn a_file_that_is_not_text_is_set_aside_like_bad_json() {
        let dir = TempDir::new("not-text");
        // One byte that is not UTF-8, as a torn write can leave a file.
        fs::write(dir.file(), [0x80]).unwrap();

        let (mut settings, warning) = Settings::load(dir.file());

        assert!(warning.is_some_and(|w| w.contains("starting fresh")));
        assert_eq!(
            fs::read(dir.0.join("settings.json.bad")).ok(),
            Some(vec![0x80])
        );
        assert!(settings.set("a", json!(1)).is_ok());
    }

    #[test]
    fn a_corrupt_file_that_cannot_be_moved_aside_is_never_overwritten() {
        let dir = TempDir::new("stuck");
        fs::write(dir.file(), "{ not json").unwrap();
        // Something already at the aside path that a file cannot replace.
        let aside = dir.0.join("settings.json.bad");
        fs::create_dir_all(aside.join("taken")).unwrap();

        let (mut settings, warning) = Settings::load(dir.file());

        assert!(warning.is_some_and(|w| w.contains("will not be saved")));
        assert!(settings.set("a", json!(1)).is_err());
        assert_eq!(fs::read_to_string(dir.file()).unwrap(), "{ not json");
    }

    #[test]
    fn an_unreadable_file_is_never_overwritten() {
        let dir = TempDir::new("unreadable");
        // A directory where the file should be cannot be read at all.
        fs::create_dir_all(dir.file()).unwrap();

        let (mut settings, warning) = Settings::load(dir.file());

        assert!(warning.is_some_and(|w| w.contains("will not be saved")));
        assert_eq!(settings.set("a", json!(1)), Err(SettingsError::ReadOnly));
        assert!(dir.file().is_dir());
    }

    #[test]
    fn a_failed_save_rolls_the_value_back() {
        let dir = TempDir::new("rollback");
        // A directory in the temporary file's place makes every save fail.
        fs::create_dir_all(dir.0.join("settings.json.tmp")).unwrap();
        let (mut settings, _) = Settings::load(dir.file());

        let result = settings.set("a", json!(1));

        assert!(matches!(result, Err(SettingsError::Io(_))));
        assert!(settings.get("a").is_none());
    }

    /// No panic on input from outside (roadmap Phase 7): the settings file
    /// is whatever is on disk, a half-written save or a stranger's edit.
    mod from_disk {
        use proptest::collection::vec;
        use proptest::prelude::*;
        use proptest::test_runner::RngSeed;

        use super::*;

        fn config() -> ProptestConfig {
            let config = ProptestConfig::default();
            if std::env::var_os("PROPTEST_RNG_SEED").is_some() {
                return config;
            }
            ProptestConfig {
                rng_seed: RngSeed::Fixed(20_261_008),
                ..config
            }
        }

        /// What a settings file could hold: any bytes, text that is almost
        /// JSON, a valid object, or one nested deeper than any parser
        /// should recurse.
        fn contents() -> impl Strategy<Value = Vec<u8>> {
            let almost = vec(
                proptest::sample::select(vec![
                    '{', '}', '[', ']', '"', ':', ',', 'a', '1', ' ', '\\',
                ]),
                0..60,
            )
            .prop_map(|chars| chars.into_iter().collect::<String>().into_bytes());
            prop_oneof![
                3 => vec(any::<u8>(), 0..200),
                3 => almost,
                1 => Just(br#"{"a": 1, "b": [true, null, "x"]}"#.to_vec()),
                1 => (1usize..20_000).prop_map(|depth| format!(r#"{{"a": {}{}}}"#, "[".repeat(depth), "]".repeat(depth)).into_bytes()),
            ]
        }

        proptest! {
            #![proptest_config(config())]

            #[test]
            fn any_file_loads_into_a_store_that_still_saves(bytes in contents()) {
                let dir = TempDir::new("from-disk");
                fs::write(dir.file(), &bytes).unwrap();
                let is_object = std::str::from_utf8(&bytes)
                    .ok()
                    .and_then(|text| serde_json::from_str::<BTreeMap<String, Value>>(text).ok())
                    .is_some();

                let (mut settings, warning) = Settings::load_with(dir.file(), Limits::DEFAULT);

                // Whatever was there, this session's choices can be saved.
                prop_assert!(settings.set("probe", Value::Bool(true)).is_ok());
                if is_object {
                    prop_assert!(warning.is_none());
                } else {
                    // Anything but a JSON object is moved aside, not lost.
                    prop_assert!(warning.is_some());
                    prop_assert_eq!(fs::read(dir.file().with_extension("json.bad")).ok(), Some(bytes));
                }
            }
        }
    }
}
