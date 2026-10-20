//! The terminal's one window and the settings store behind it.
//!
//! The front end owns layouts: what panels exist, where, and with what
//! parameters. The shell persists whatever the front end asks it to.
//! Window size and position are restored by the window-state plugin.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::thread;
use std::time::Duration;

use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager, RunEvent, State, Window, WindowEvent};

use crate::settings::Settings;

pub const MAIN_WINDOW: &str = "main";
const SETTINGS_FILE: &str = "settings.json";
/// Sent to the window before quitting, so changes still waiting in a
/// debounced save reach the store.
const FLUSH_EVENT: &str = "workspace-flush";
/// How long the window gets to flush before the app exits. A save is one
/// local IPC call and a small file write.
const EXIT_GRACE: Duration = Duration::from_millis(300);

pub struct Store(Mutex<Settings>);

/// Set once the app is shutting down, so it shuts down once.
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

/// Saves the window's changes, then quits or restarts: once, however many
/// ways it is asked.
fn leave(app: &AppHandle, then: fn(&AppHandle)) {
    if let Some(lifecycle) = app.try_state::<Lifecycle>() {
        if lifecycle.exiting.swap(true, Ordering::SeqCst) {
            return;
        }
    }
    if let Err(e) = app.emit(FLUSH_EVENT, ()) {
        crate::logbook::warn(&format!("could not ask the window to save first: {e}"));
    }
    let app = app.clone();
    thread::spawn(move || {
        thread::sleep(EXIT_GRACE);
        then(&app);
    });
}

/// Restarts the app the way quitting ends it: the window saves first.
pub(crate) fn restart(app: &AppHandle) {
    leave(app, |app| app.restart());
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

/// Loads settings.
pub fn setup(app: &tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let path = app.path().app_config_dir()?.join(SETTINGS_FILE);
    let (settings, warning) = Settings::load(path);
    if let Some(warning) = warning {
        crate::logbook::warn(&warning);
    }
    app.manage(Store(Mutex::new(settings)));
    app.manage(Lifecycle::default());
    Ok(())
}

/// Closing the window quits the app, after giving it a moment to flush
/// pending saves.
pub fn on_window_event(window: &Window, event: &WindowEvent) {
    if let WindowEvent::CloseRequested { api, .. } = event {
        if window.label() == MAIN_WINDOW {
            api.prevent_close();
            leave(window.app_handle(), |app| app.exit(0));
        }
    }
}

/// Any way of quitting (the window, the OS ending the session) counts as
/// exiting, so it happens once.
pub fn on_run_event(app: &AppHandle, event: RunEvent) {
    if let RunEvent::ExitRequested { .. } = event {
        if let Some(lifecycle) = app.try_state::<Lifecycle>() {
            lifecycle.exiting.store(true, Ordering::SeqCst);
        }
    }
}
