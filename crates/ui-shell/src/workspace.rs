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
const MIN_WINDOW_SIZE: (f64, f64) = (480.0, 320.0);

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
#[tauri::command]
pub async fn open_window(app: AppHandle, label: String) -> Result<(), String> {
    if !is_extra_window(&label) {
        return Err(format!("invalid window label \"{label}\""));
    }
    if let Some(existing) = app.get_webview_window(&label) {
        return existing.set_focus().map_err(|e| e.to_string());
    }
    let store = app.state::<Store>();
    record_window(&store, &label, true)?;
    build_window(&app, &label).map_err(|e| {
        if let Err(undo) = record_window(&store, &label, false) {
            eprintln!("could not un-record window {label}: {undo}");
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

fn build_window(app: &AppHandle, label: &str) -> tauri::Result<()> {
    let (width, height) = NEW_WINDOW_SIZE;
    let (min_width, min_height) = MIN_WINDOW_SIZE;
    WebviewWindowBuilder::new(app, label, WebviewUrl::App("index.html".into()))
        .title(EXTRA_WINDOW_TITLE)
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
        eprintln!("{warning}");
    }
    let reopen = listed_windows(settings.get(WINDOWS_KEY));
    app.manage(Store(Mutex::new(settings)));
    app.manage(Lifecycle::default());

    for label in reopen {
        if let Err(e) = build_window(app.handle(), &label) {
            eprintln!("could not reopen window {label}: {e}");
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
                eprintln!("could not ask windows to save before quitting: {e}");
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
                eprintln!("could not forget window {}: {e}", window.label());
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
