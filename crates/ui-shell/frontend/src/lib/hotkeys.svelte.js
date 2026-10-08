// Actions a key can run, and which key runs each. Actions are registered
// with a default key, the way screens are, so a plugin can add its own.
// The trader's choices are saved in the settings store (`ui.hotkeys`) and
// apply in every window. One keydown listener per window runs them.

import { emit, listen } from "@tauri-apps/api/event";
import { debouncedSetter, getSetting } from "./settings.js";
import { actionFor, bindingsFrom, choicesOf, comboOf, isBindable, rebound } from "./hotkeys.js";

const SETTING = "ui.hotkeys";
const EVENT = "hotkeys-change";
const SAVE_DELAY_MS = 0;
const ACTION_ID = /^[a-z][a-z0-9-]*(\.[a-z][a-z0-9-]*)+$/i;

const registered = new Map();

class Hotkeys {
  /** Registered actions, in registration order: `{ id, title, defaultKey }`. */
  actions = $state.raw([]);
  /** `{ actionId: combo | null }`. */
  bindings = $state.raw({});
  /** While a key is being recorded for an action, presses do not run actions. */
  recording = $state(false);
}

export const hotkeys = new Hotkeys();

let saved = null;
const save = debouncedSetter(SETTING, SAVE_DELAY_MS);
/** Shows the trader what an action reports; set by `startHotkeys`. */
let report = () => {};

/**
 * Registers an action a key can run. `id` is namespaced (`ticket.buy`),
 * `title` a message id, `defaultKey` a combo or null for none. `run`
 * returns nothing, or a notice `{ id, vars, count }` to show the trader,
 * and may be async; a thrown error is shown too.
 */
export function registerAction({ id, title, defaultKey = null, run }) {
  if (!ACTION_ID.test(id ?? "")) throw new Error(`hotkey action id "${id}" must be namespaced, like "ticket.buy"`);
  if (typeof run !== "function") throw new Error(`hotkey action "${id}" needs a run function`);
  registered.set(id, { id, title, defaultKey, run });
  hotkeys.actions = [...registered.values()].map(({ id: key, title: name, defaultKey: combo }) => ({
    id: key,
    title: name,
    defaultKey: combo,
  }));
  hotkeys.bindings = bindingsFrom(hotkeys.actions, saved);
}

/** The combo that runs `actionId`, or null: for hints next to the controls. */
export function keyOf(actionId) {
  return hotkeys.bindings[actionId] ?? null;
}

const sameChoices = (a, b) => JSON.stringify(a ?? {}) === JSON.stringify(b ?? {});

function apply(bindings) {
  hotkeys.bindings = bindings;
  saved = choicesOf(hotkeys.actions, bindings);
  save(saved);
  // Saved either way; other windows that missed the event read it at their next start.
  emit(EVENT, saved).catch((err) => report({ id: "hotkey.notShared", vars: { error: String(err) } }));
}

/**
 * Gives `actionId` the combo, taking it off any action that had it; null
 * unbinds. A combo no shortcut may use (see `isBindable`) is refused:
 * returns false and changes nothing.
 */
export function bindKey(actionId, combo) {
  if (combo !== null && !isBindable(combo)) return false;
  apply(rebound(hotkeys.bindings, actionId, combo));
  return true;
}

/** Every action back to its default key. */
export function resetKeys() {
  apply(bindingsFrom(hotkeys.actions, null));
}

/** Writes a pending change now; see the shell's quit sequence. */
export function flushHotkeys() {
  return save.flush();
}

async function run(action) {
  try {
    const notice = await action.run();
    if (notice) report(notice);
  } catch (err) {
    report({ id: "hotkey.failed", vars: { error: String(err) } });
  }
}

/** Runs a registered action as its key would, from a menu or a search; unknown ids do nothing. */
export function runAction(actionId) {
  const action = registered.get(actionId);
  return action ? run(action) : Promise.resolve();
}

/** A dialog or menu open owns the keyboard: shortcuts wait until it closes. */
const somethingModal = () => document.querySelector("dialog[open], [role='menu'], [role='dialog'], [data-popup]") !== null;

function onKeydown(event) {
  if (hotkeys.recording || event.defaultPrevented || event.repeat || somethingModal()) return;
  const combo = comboOf(event);
  if (!combo) return;
  const action = registered.get(actionFor(hotkeys.bindings, combo));
  if (!action) return;
  event.preventDefault();
  run(action);
}

let started = null;

/**
 * Loads the trader's keys, follows other windows' changes, and starts
 * listening for presses; once per window. `onNotice` shows what an action
 * reports.
 */
export function startHotkeys({ onNotice } = {}) {
  started ??= (async () => {
    report = onNotice ?? report;
    // A change from another window while the store is read is newer than
    // what the store held, so it wins. A window's own change comes back
    // too; it is already applied.
    let heard = false;
    await listen(EVENT, ({ payload }) => {
      if (sameChoices(payload, saved)) return;
      heard = true;
      saved = payload;
      hotkeys.bindings = bindingsFrom(hotkeys.actions, saved);
    });
    const stored = await getSetting(SETTING);
    if (!heard) {
      saved = stored;
      hotkeys.bindings = bindingsFrom(hotkeys.actions, saved);
    }
    window.addEventListener("keydown", onKeydown);
  })();
  return started;
}
