// The shell's persistent settings store: string keys to JSON values.
// Keys are lower-case and namespaced by convention: `workspace.*` for the
// workspace, `ui.*` for app-wide preferences, `screen.<id>.*` for a
// screen's own settings.

import { invoke } from "@tauri-apps/api/core";

export function getSetting(key) {
  return invoke("settings_get", { key });
}

export function setSetting(key, value) {
  return invoke("settings_set", { key, value });
}

export function removeSetting(key) {
  return invoke("settings_remove", { key });
}

/**
 * A setter that writes at most once per `delayMs`, keeping only the latest
 * value: layouts change on every pixel of a drag, the disk needs only the
 * result. `onError` receives a failed write's message. `set.flush()` writes
 * a pending value now (before quitting) and resolves when it is stored.
 */
export function debouncedSetter(key, delayMs, onError) {
  let timer = null;
  let pending = false;
  let latest;

  function write() {
    clearTimeout(timer);
    timer = null;
    if (!pending) return Promise.resolve();
    pending = false;
    return setSetting(key, latest).catch((err) => onError?.(String(err)));
  }

  const set = (value) => {
    latest = value;
    pending = true;
    clearTimeout(timer);
    timer = setTimeout(write, delayMs);
  };
  set.flush = write;
  return set;
}
