// The menu bar's extension point (ADR 0017). Menus hold commands, and most
// commands are registered actions (hotkeys.svelte.js): the menu shows the
// key the trader bound, and a plugin's action sits beside the built-ins with
// no change here. Two other kinds fill the gaps: a command a key must not
// take over (Copy: Ctrl+C belongs to the field that has focus), and a list
// of choices (a theme, a text size), picked rather than bound. Register
// before the app mounts, as screens are: the menu bar reads the registry
// once.

const ID = /^[a-z][a-z0-9-]{0,39}$/;
const DEFAULT_ORDER = 50;

const menus = new Map();

/**
 * Registers a menu on the menu bar. The built-ins sit at 10 (File) to 90
 * (Help); a plugin's menu picks a number between.
 *
 * @param {{ id: string, title: string, order?: number }} menu
 *   `title` is a message id (see src/i18n) or plain text.
 */
export function registerMenu({ id, title, order = DEFAULT_ORDER } = {}) {
  if (typeof id !== "string" || !ID.test(id)) {
    throw new Error(`menu id "${id}" must be 1-40 of a-z, 0-9, '-', starting with a letter`);
  }
  if (menus.has(id)) throw new Error(`menu "${id}" is already registered`);
  if (typeof title !== "string" || !title) throw new Error(`menu "${id}" needs a title`);
  menus.set(id, { id, title, order: Number.isFinite(order) ? order : DEFAULT_ORDER, groups: new Map() });
}

/**
 * Adds an item to a menu, in `group`. Groups show in the order they were
 * first used, with a rule between them. An item is exactly one of:
 * - `{ action }`: a registered action's id; its title and key come from it;
 * - `{ title, run, hint? }`: a command no key is bound to; `hint` names
 *   the key that does it natively, such as "Ctrl+C";
 * - `{ title, choices }`: a submenu; `choices()` returns
 *   `[{ id, label, checked?, pick }]` each time it opens, `label` being
 *   the text to show, already in the trader's language. With `checked`
 *   the choices are one-of (a theme); without, each is a command (a
 *   screen to add).
 *
 * A command's `run` and a choice's `pick` may return a notice
 * `{ id, vars }` to show the trader, as an action's `run` may.
 *
 * @param {string} menuId
 * @param {object} item
 */
export function registerMenuItem(menuId, { group = "main", action, title, run, hint = null, choices } = {}) {
  const menu = menus.get(menuId);
  if (!menu) throw new Error(`there is no menu "${menuId}" to add to`);
  const kind = action !== undefined ? "action" : run !== undefined ? "command" : choices !== undefined ? "choices" : null;
  const given = [action, run, choices].filter((part) => part !== undefined).length;
  const wellFormed =
    (kind === "action" && typeof action === "string" && action !== "") ||
    (kind === "command" && typeof run === "function") ||
    (kind === "choices" && typeof choices === "function");
  if (given !== 1 || !wellFormed) {
    throw new Error(`a menu item is an action, a command, or choices, and only one of them`);
  }
  if (kind !== "action" && (typeof title !== "string" || !title)) {
    throw new Error(`a "${menuId}" menu item needs a title`);
  }
  const item =
    kind === "action" ? { kind, action } : kind === "command" ? { kind, title, run, hint } : { kind, title, choices };
  const items = menu.groups.get(group) ?? [];
  menu.groups.set(group, [...items, Object.freeze(item)]);
}

/** The menus in order, each with its items in groups. */
export function listMenus() {
  return [...menus.values()]
    .sort((a, b) => a.order - b.order)
    .map(({ id, title, groups }) => ({ id, title, groups: [...groups.values()].map((items) => [...items]) }));
}

/** Test-only: forget every menu. */
export function resetMenusForTests() {
  menus.clear();
}
