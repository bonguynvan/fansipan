// The sidebar's places: Trade, the workspace, first; then each
// section registered the way screens are. A section is registered only once
// it has something to show, so the sidebar never offers an empty place.
// Each one is also an action ("section.<id>"), so a key can be bound to it
// and search finds it.

import { tick } from "svelte";
import { registerAction } from "./hotkeys.svelte.js";

const ID = /^[a-z][a-z0-9-]{0,39}$/;
const DEFAULT_ORDER = 50;

/** The workspace's own section: always there, always first. */
export const TRADE = "trade";

const sections = new Map();

class Sections {
  /** The section shown in the main window. */
  current = $state(TRADE);
}

export const place = new Sections();

/**
 * Registers a section. `title` is a message id; `icon` an SVG path in a
 * 24-unit box; `component` what the section shows (none for Trade, which is
 * the workspace); `order` places it, Trade being 0.
 *
 * @param {{ id: string, title: string, icon: string, component?: object, order?: number }} section
 */
export function registerSection({ id, title, icon, component = null, order = DEFAULT_ORDER } = {}) {
  if (typeof id !== "string" || !ID.test(id)) {
    throw new Error(`section id "${id}" must be 1-40 of a-z, 0-9, '-', starting with a letter`);
  }
  if (sections.has(id)) throw new Error(`section "${id}" is already registered`);
  if (typeof title !== "string" || !title) throw new Error(`section "${id}" needs a title`);
  if (id !== TRADE && !component) throw new Error(`section "${id}" needs a component`);
  sections.set(id, Object.freeze({ id, title, icon, component, order: id === TRADE ? 0 : order }));
  registerAction({ id: `section.${id}`, title, run: () => showSection(id) });
}

/** The sections in order. */
export function listSections() {
  return [...sections.values()].sort((a, b) => a.order - b.order);
}

export function getSection(id) {
  return sections.get(id);
}

/** Shows section `id`; one not registered leaves the current one shown. */
export function showSection(id) {
  if (sections.has(id)) place.current = id;
}

/**
 * Shows the workspace for an action that works on it: any action but the
 * app's own (search, shortcuts, about, full screen, exit), the trader's
 * switch, and the sections themselves, which make sense wherever the
 * trader is.
 */
export function revealFor(actionId) {
  const anywhere = ["app.", "trading.", "section."].some((prefix) => actionId.startsWith(prefix));
  if (anywhere || place.current === TRADE) return undefined;
  showSection(TRADE);
  // Drawn before the action runs: one that measures panels needs them shown.
  return tick();
}

/** Test-only: forget every section. */
export function resetSectionsForTests() {
  sections.clear();
  place.current = TRADE;
}
