//! The workspace's windows and the settings store behind them.
//!
//! The front end owns layouts: what panels exist, where, and with what
//! parameters. The shell only keeps the list of extra windows, so it can
//! reopen them at start, and persists whatever the front end asks it to.
//! Window size and position are restored by the window-state plugin.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::thread;
use std::time::Duration;

use serde_json::Value;
use tauri::{
    AppHandle, Emitter, Manager, RunEvent, State, WebviewUrl, WebviewWindowBuilder, Window,
    WindowEvent,
};

use crate::settings::Settings;

pub const MAIN_WINDOW: &str = "main";
const EXTRA_WINDOW_PREFIX: &str = "ws-";
const MAX_EXTRA_LABEL: usize = 24;
const WINDOWS_KEY: &str = "workspace.windows";
const LAYOUT_PREFIX: &str = "workspace.layout.";
const SETTINGS_FILE: &str = "settings.json";
/// Sent to every window before quitting, so changes still waiting in a
/// debounced save reach the store.
const FLUSH_EVENT: &str = "workspace-flush";
/// How long windows get to flush before the app exits. A save is one local
/// IPC call and a small file write.
const EXIT_GRACE: Duration = Duration::from_millis(300);
/// Distinct from the main window's title so the two can be told apart in
/// the taskbar and window switcher.
const EXTRA_WINDOW_TITLE: &str = "Fansipan · extra window";
const NEW_WINDOW_SIZE: (f64, f64) = (960.0, 640.0);
/// Small enough for one narrow screen, such as a ticket, on its own.
const MIN_WINDOW_SIZE: (f64, f64) = (240.0, 160.0);
const MAX_WINDOW_SIZE: f64 = 8192.0;

pub struct Store(Mutex<Settings>);

/// Set once the app is shutting down, so the extra windows it closes on
/// the way out stay in the list and come back next start.
#[derive(Default)]
pub struct Lifecycle {
    exiting: AtomicBool,
}

impl Store {
    fn lock(&self) -> Result<MutexGuard<'_, Settings>, String> {
        self.0
            .lock()
            .map_err(|_| "settings are unavailable after an internal error; restart".to_string())
    }

    /// One setting, for the shell's own use; `None` when unset or the store
    /// is unavailable.
    pub(crate) fn read(&self, key: &str) -> Option<Value> {
        self.lock().ok()?.get(key).cloned()
    }

    pub(crate) fn write(&self, key: &str, value: Value) -> Result<(), String> {
        self.lock()?.set(key, value).map_err(|e| e.to_string())
    }
}

/// Restarts the app the way quitting ends it: every window is asked to save
/// and given a moment, and the extra windows stay listed for next start.
pub(crate) fn restart(app: &AppHandle) {
    if let Some(lifecycle) = app.try_state::<Lifecycle>() {
        if lifecycle.exiting.swap(true, Ordering::SeqCst) {
            return;
        }
    }
    if let Err(e) = app.emit(FLUSH_EVENT, ()) {
        crate::logbook::warn(&format!(
            "could not ask windows to save before restarting: {e}"
        ));
    }
    let app = app.clone();
    thread::spawn(move || {
        thread::sleep(EXIT_GRACE);
        app.restart();
    });
}

/// `ws-` followed by 1-24 of a-z and 0-9. Labels become settings keys and
/// window identifiers, so nothing else is accepted from the front end.
pub fn is_extra_window(label: &str) -> bool {
    label.strip_prefix(EXTRA_WINDOW_PREFIX).is_some_and(|rest| {
        !rest.is_empty()
            && rest.len() <= MAX_EXTRA_LABEL
            && rest
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
    })
}

/// The extra windows recorded in settings: valid labels only, no repeats,
/// in the order they were opened.
fn listed_windows(value: Option<&Value>) -> Vec<String> {
    let mut labels: Vec<String> = Vec::new();
    for label in value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
    {
        if is_extra_window(label) && !labels.iter().any(|l| l == label) {
            labels.push(label.to_string());
        }
    }
    labels
}

fn with_window(listed: &[String], label: &str) -> Vec<String> {
    let mut next: Vec<String> = listed.iter().filter(|l| *l != label).cloned().collect();
    next.push(label.to_string());
    next
}

fn without_window(listed: &[String], label: &str) -> Vec<String> {
    listed.iter().filter(|l| *l != label).cloned().collect()
}

fn layout_key(label: &str) -> String {
    format!("{LAYOUT_PREFIX}{label}")
}

#[tauri::command]
pub fn settings_get(store: State<'_, Store>, key: String) -> Result<Option<Value>, String> {
    Ok(store.lock()?.get(&key).cloned())
}

#[tauri::command]
pub fn settings_set(store: State<'_, Store>, key: String, value: Value) -> Result<(), String> {
    store.lock()?.set(&key, value).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn settings_remove(store: State<'_, Store>, key: String) -> Result<(), String> {
    store.lock()?.remove(&key).map_err(|e| e.to_string())
}

/// Opens (or focuses) an extra workspace window and records it so it
/// reopens at the next start. Recorded first and un-recorded if the window
/// cannot be built, so the list never names a window that does not exist
/// and never misses one that does. Async because building a window from a
/// synchronous command can deadlock on Windows.
///
/// `width` and `height` (logical pixels) open it at the size of the panel
/// that leaves for it.
#[tauri::command]
pub async fn open_window(
    app: AppHandle,
    label: String,
    width: Option<f64>,
    height: Option<f64>,
) -> Result<(), String> {
    if !is_extra_window(&label) {
        return Err(format!("invalid window label \"{label}\""));
    }
    if let Some(existing) = app.get_webview_window(&label) {
        return existing.set_focus().map_err(|e| e.to_string());
    }
    let store = app.state::<Store>();
    record_window(&store, &label, true)?;
    build_window(&app, &label, window_size(width, height)).map_err(|e| {
        if let Err(undo) = record_window(&store, &label, false) {
            crate::logbook::warn(&format!("could not un-record window {label}: {undo}"));
        }
        e.to_string()
    })
}

fn record_window(store: &Store, label: &str, open: bool) -> Result<(), String> {
    let mut settings = store.lock()?;
    let listed = listed_windows(settings.get(WINDOWS_KEY));
    let next = if open {
        with_window(&listed, label)
    } else {
        without_window(&listed, label)
    };
    settings
        .set(WINDOWS_KEY, Value::from(next))
        .map_err(|e| e.to_string())
}

/// The size a new extra window opens at: the one asked for, kept within
/// bounds, or a default when none was asked.
fn window_size(width: Option<f64>, height: Option<f64>) -> (f64, f64) {
    let fit = |asked: Option<f64>, default: f64, min: f64| {
        asked
            .filter(|v| v.is_finite())
            .map_or(default, |v| v.clamp(min, MAX_WINDOW_SIZE))
    };
    (
        fit(width, NEW_WINDOW_SIZE.0, MIN_WINDOW_SIZE.0),
        fit(height, NEW_WINDOW_SIZE.1, MIN_WINDOW_SIZE.1),
    )
}

/// An extra window is the screen alone: no title bar or frame of the
/// system's (the front end gives it a drag strip), but the system's shadow
/// and edge resizing. The title still names it in the taskbar.
fn build_window(app: &AppHandle, label: &str, (width, height): (f64, f64)) -> tauri::Result<()> {
    let (min_width, min_height) = MIN_WINDOW_SIZE;
    WebviewWindowBuilder::new(app, label, WebviewUrl::App("index.html".into()))
        .title(EXTRA_WINDOW_TITLE)
        .decorations(false)
        .shadow(true)
        .inner_size(width, height)
        .min_inner_size(min_width, min_height)
        .build()
        .map(drop)
}

/// Loads settings and reopens the extra windows open at last exit.
pub fn setup(app: &tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let path = app.path().app_config_dir()?.join(SETTINGS_FILE);
    let (settings, warning) = Settings::load(path);
    if let Some(warning) = warning {
        crate::logbook::warn(&warning);
    }
    let reopen = listed_windows(settings.get(WINDOWS_KEY));
    app.manage(Store(Mutex::new(settings)));
    app.manage(Lifecycle::default());

    for label in reopen {
        if let Err(e) = build_window(app.handle(), &label, NEW_WINDOW_SIZE) {
            crate::logbook::warn(&format!("could not reopen window {label}: {e}"));
        }
    }
    Ok(())
}

/// Closing the main window quits the app, after giving every window a
/// moment to flush pending saves; closing an extra window forgets it and its
/// layout, unless the app is quitting anyway.
pub fn on_window_event(window: &Window, event: &WindowEvent) {
    let app = window.app_handle();
    let lifecycle = app.state::<Lifecycle>();
    match event {
        WindowEvent::CloseRequested { api, .. } if window.label() == MAIN_WINDOW => {
            api.prevent_close();
            if lifecycle.exiting.swap(true, Ordering::SeqCst) {
                return;
            }
            if let Err(e) = app.emit(FLUSH_EVENT, ()) {
                crate::logbook::warn(&format!(
                    "could not ask windows to save before quitting: {e}"
                ));
            }
            let app = app.clone();
            thread::spawn(move || {
                thread::sleep(EXIT_GRACE);
                app.exit(0);
            });
        }
        WindowEvent::Destroyed
            if is_extra_window(window.label()) && !lifecycle.exiting.load(Ordering::SeqCst) =>
        {
            if let Err(e) = forget_window(&app.state::<Store>(), window.label()) {
                crate::logbook::warn(&format!("could not forget window {}: {e}", window.label()));
            }
        }
        _ => {}
    }
}

/// Any way of quitting (the main window, the OS ending the session) counts
/// as exiting, so the extra windows it closes stay listed for next start.
pub fn on_run_event(app: &AppHandle, event: RunEvent) {
    if let RunEvent::ExitRequested { .. } = event {
        if let Some(lifecycle) = app.try_state::<Lifecycle>() {
            lifecycle.exiting.store(true, Ordering::SeqCst);
        }
    }
}

fn forget_window(store: &Store, label: &str) -> Result<(), String> {
    record_window(store, label, false)?;
    store
        .lock()?
        .remove(&layout_key(label))
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_new_window_opens_at_the_size_asked_within_bounds() {
        assert_eq!(window_size(Some(320.0), Some(480.0)), (320.0, 480.0));
        assert_eq!(window_size(None, None), NEW_WINDOW_SIZE);
        assert_eq!(
            window_size(Some(10.0), Some(1e9)),
            (MIN_WINDOW_SIZE.0, MAX_WINDOW_SIZE)
        );
        assert_eq!(
            window_size(Some(f64::NAN), Some(f64::INFINITY)),
            NEW_WINDOW_SIZE
        );
    }

    #[test]
    fn only_ws_labels_are_extra_windows() {
        assert!(is_extra_window("ws-1"));
        assert!(is_extra_window("ws-k3x9a"));
        for bad in [
            "main",
            "ws-",
            "ws-UP",
            "ws-a/b",
            "ws-a b",
            "x-1",
            &format!("ws-{}", "a".repeat(25)),
        ] {
            assert!(!is_extra_window(bad), "{bad:?}");
        }
    }

    #[test]
    fn listed_windows_keeps_valid_unique_labels_in_order() {
        let value = json!(["ws-2", "main", "ws-1", 7, "ws-2", "../x"]);

        assert_eq!(listed_windows(Some(&value)), vec!["ws-2", "ws-1"]);
        assert!(listed_windows(Some(&json!({ "not": "a list" }))).is_empty());
        assert!(listed_windows(None).is_empty());
    }

    #[test]
    fn adding_and_removing_windows_keeps_the_list_unique() {
        let listed = vec!["ws-1".to_string(), "ws-2".to_string()];

        assert_eq!(with_window(&listed, "ws-1"), vec!["ws-2", "ws-1"]);
        assert_eq!(with_window(&listed, "ws-3"), vec!["ws-1", "ws-2", "ws-3"]);
        assert_eq!(without_window(&listed, "ws-1"), vec!["ws-2"]);
    }

    #[test]
    fn a_window_layout_lives_under_its_label() {
        assert_eq!(layout_key("ws-1"), "workspace.layout.ws-1");
    }
}
