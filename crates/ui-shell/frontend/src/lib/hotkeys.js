// Key bindings as data. A combo is a string like "F1" or "Ctrl+Shift+X":
// modifiers in a fixed order, then the key. Letters and digits are named
// by their place on the keyboard (KeyX is X), so a binding works whatever
// layout or input method (Telex, VNI) is on. Nothing here listens or
// saves; see hotkeys.svelte.js.

const MODIFIERS = ["Ctrl", "Alt", "Shift", "Meta"];
const MODIFIER_KEYS = new Set(["Control", "Alt", "AltGraph", "Shift", "Meta", "OS"]);
const NAMED_KEYS = new Set([
  "Escape",
  "Enter",
  "Tab",
  "Space",
  "Backspace",
  "Delete",
  "Insert",
  "Home",
  "End",
  "PageUp",
  "PageDown",
  "ArrowUp",
  "ArrowDown",
  "ArrowLeft",
  "ArrowRight",
]);
const FUNCTION_KEY = /^F([1-9]|1[0-9]|2[0-4])$/;

/** The key of a press, by place for letters and digits; null for a modifier alone. */
function keyName(event) {
  const code = event.code ?? "";
  if (/^Key[A-Z]$/.test(code)) return code.slice(3);
  if (/^Digit[0-9]$/.test(code)) return code.slice(5);
  if (FUNCTION_KEY.test(code)) return code;
  const key = event.key;
  if (!key || MODIFIER_KEYS.has(key) || key === "Dead" || key === "Process") return null;
  if (key === " ") return "Space";
  return key.length === 1 ? key.toUpperCase() : key;
}

/**
 * Whether a press is AltGr typing a character. Windows reports AltGr as
 * Ctrl+Alt, so AltGr+X typing "ź" on a Polish layout would otherwise read
 * as Ctrl+Alt+X.
 */
function typesWithAltGraph(event) {
  if (event.getModifierState?.("AltGraph")) return true;
  if (!(event.ctrlKey && event.altKey) || event.key?.length !== 1) return false;
  const letter = /^Key([A-Z])$/.exec(event.code ?? "")?.[1];
  return letter !== undefined && event.key.toUpperCase() !== letter;
}

/** The combo a key press makes, or null while composing text, for AltGr typing, or for a modifier alone. */
export function comboOf(event) {
  if (event.isComposing || typesWithAltGraph(event)) return null;
  const key = keyName(event);
  if (!key) return null;
  const held = [event.ctrlKey && "Ctrl", event.altKey && "Alt", event.shiftKey && "Shift", event.metaKey && "Meta"];
  return [...held.filter(Boolean), key].join("+");
}

/** A combo as written by a person or read from the store, in canonical form; null if it is not one. */
export function normalizeCombo(text) {
  if (typeof text !== "string") return null;
  const parts = text.split("+").map((part) => part.trim());
  const key = parts.pop();
  if (!key) return null;
  const held = new Set();
  for (const part of parts) {
    const modifier = MODIFIERS.find((m) => m.toLowerCase() === part.toLowerCase());
    if (!modifier || held.has(modifier)) return null;
    held.add(modifier);
  }
  const named = [...NAMED_KEYS].find((k) => k.toLowerCase() === key.toLowerCase());
  const canonical =
    key.length === 1 && /[a-z0-9]/i.test(key)
      ? key.toUpperCase()
      : FUNCTION_KEY.test(key.toUpperCase())
        ? key.toUpperCase()
        : named;
  if (!canonical) return null;
  return [...MODIFIERS.filter((m) => held.has(m)), canonical].join("+");
}

/**
 * Whether a combo can be a shortcut: a function key, or one held with
 * Ctrl, Alt, or Meta. A plain letter, Enter, or Space belongs to the field
 * or button that has focus, so it is never taken, and every shortcut can
 * therefore run while the trader types.
 */
export function isBindable(combo) {
  const parts = combo.split("+");
  return FUNCTION_KEY.test(parts.at(-1)) || parts.some((p) => p === "Ctrl" || p === "Alt" || p === "Meta");
}

/** `bindings` with `combo` on `actionId`, taken off any other action that had it. Null unbinds. */
export function rebound(bindings, actionId, combo) {
  const freed = Object.fromEntries(
    Object.entries(bindings).map(([id, key]) => [id, combo !== null && key === combo ? null : key]),
  );
  return { ...freed, [actionId]: combo };
}

/**
 * Each action's combo: its default, unless the trader's saved choices
 * (`{ actionId: combo | null }`, read from the store) say otherwise. A saved
 * choice for an unknown action or with an unreadable combo is ignored.
 */
export function bindingsFrom(actions, saved) {
  const defaults = Object.fromEntries(actions.map((a) => [a.id, normalizeCombo(a.defaultKey)]));
  const known = new Set(actions.map((a) => a.id));
  const choices = saved && typeof saved === "object" && !Array.isArray(saved) ? Object.entries(saved) : [];
  return choices.reduce((bindings, [id, combo]) => {
    if (!known.has(id)) return bindings;
    if (combo === null) return rebound(bindings, id, null);
    const clean = normalizeCombo(combo);
    return clean && isBindable(clean) ? rebound(bindings, id, clean) : bindings;
  }, defaults);
}

/** What to save: only the trader's departures from the defaults, so a new default still reaches untouched actions. */
export function choicesOf(actions, bindings) {
  return Object.fromEntries(
    actions
      .filter((a) => (bindings[a.id] ?? null) !== normalizeCombo(a.defaultKey))
      .map((a) => [a.id, bindings[a.id] ?? null]),
  );
}

/** The action `combo` runs, or null. */
export function actionFor(bindings, combo) {
  return Object.entries(bindings).find(([, key]) => key === combo)?.[0] ?? null;
}
