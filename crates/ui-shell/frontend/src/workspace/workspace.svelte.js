// One window's workspace: which panels it shows, where, and how they are
// linked. The layout is dockview's own JSON, saved per window in the
// shell's settings store; the shell reopens extra windows at start and each
// window restores itself from its key.

import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { getLayout, getScreen } from "./registry.js";
import { DEFAULT_LAYOUT } from "./builtins.js";
import { debouncedSetter, getSetting, removeSetting, setSetting } from "../lib/settings.js";
import { isLinked, links, nextGroup } from "../lib/links.svelte.js";
import { newWindowLabel } from "./labels.js";
import { t } from "../i18n/i18n.svelte.js";

export const MAIN_WINDOW = "main";
/** Bump when the saved shape changes; older saves then fall back to a preset. */
export const LAYOUT_VERSION = 1;
const SAVE_DELAY_MS = 400;

class WorkspaceState {
  api = $state.raw(null);
  label = $state(MAIN_WINDOW);
  /** `{ id, vars }` of a message for the notice bar, or null. */
  notice = $state(null);
  /**
   * True while a whole layout is being replaced or restored. Panels come and
   * go in bulk then, so "the last panel left" means nothing: an extra window
   * must not close itself mid-rebuild.
   */
  rebuilding = false;
}

export const workspace = new WorkspaceState();

let saver = null;

export function layoutKey(label) {
  return `workspace.layout.${label}`;
}

export function currentWindowLabel() {
  try {
    return getCurrentWindow().label;
  } catch {
    return MAIN_WINDOW;
  }
}

function newPanelId(screenId) {
  return `${screenId}-${crypto.randomUUID().slice(0, 8)}`;
}

/** What a layout saves: dockview's JSON, versioned. */
export function serialize(api) {
  return { version: LAYOUT_VERSION, dockview: api.toJSON() };
}

/** Saves this window's layout soon; safe to call on every change. */
export function persist() {
  if (workspace.api) saver?.(serialize(workspace.api));
}

/** Writes a pending layout save now; see the shell's quit sequence. */
export function flushLayout() {
  return saver?.flush() ?? Promise.resolve();
}

function rebuild(change) {
  workspace.rebuilding = true;
  try {
    change();
  } finally {
    workspace.rebuilding = false;
  }
}

function notify(id, vars) {
  workspace.notice = { id, vars };
}

/**
 * Adds a panel showing a registered screen. An unregistered id still gets a
 * panel, which explains what is missing instead of failing the whole layout.
 * `options.title` (a message id or plain text) overrides the screen's title
 * and is kept in the panel's `titleKey` parameter, so the tab can be
 * re-labelled in another language.
 */
export function addScreen(screenId, options = {}) {
  const api = workspace.api;
  if (!api) return null;
  const screen = getScreen(screenId);
  const params = { ...(screen?.defaultParams ?? {}), ...(options.params ?? {}) };
  if (options.title) params.titleKey = options.title;
  const panel = {
    id: newPanelId(screenId),
    component: screenId,
    title: t(options.title ?? screen?.title ?? screenId),
    params,
  };
  if (options.position) panel.position = options.position;
  return api.addPanel(panel);
}

/**
 * Replaces this window's panels with a registered layout. Size hints
 * (`initialWidth`, `initialHeight`) are applied once every panel is in:
 * dockview splits evenly while a layout is still being built.
 */
export function applyLayout(layoutId) {
  const api = workspace.api;
  const layout = getLayout(layoutId) ?? getLayout(DEFAULT_LAYOUT);
  if (!api || !layout) return;
  rebuild(() => {
    api.clear();
    const sized = [];
    layout.build((screenId, options = {}) => {
      const { initialWidth, initialHeight, ...rest } = options;
      const panel = addScreen(screenId, rest);
      const size = { ...(initialWidth && { width: initialWidth }), ...(initialHeight && { height: initialHeight }) };
      if (panel && Object.keys(size).length) sized.push([panel, size]);
      return panel;
    });
    for (const [panel, size] of sized) panel.group.api.setSize(size);
  });
  persist();
}

/**
 * Labels every tab in the current language. A saved layout holds tab text
 * in whatever language it was saved in; the source of each title is the
 * panel's `titleKey` or its screen's title.
 */
export function retitlePanels() {
  for (const panel of workspace.api?.panels ?? []) {
    const key = panel.params?.titleKey ?? screenOf(panel)?.title;
    if (key) panel.api.setTitle(t(key));
  }
}

/**
 * Restores this window from its saved layout: dockview JSON from an earlier
 * session, or a seed (a panel list) written by the window that popped a
 * panel out. With nothing usable, the main window shows the default layout;
 * an extra window shows a watchlist if its layout was damaged, and closes
 * if it never had one (it would otherwise reopen, empty, forever).
 */
export async function restore(api, label) {
  workspace.api = api;
  workspace.label = label;
  // Saving stays off until the saved layout is in: a resize or an empty
  // first layout must never overwrite it while it is still loading.
  saver = null;

  let saved;
  try {
    saved = await getSetting(layoutKey(label));
  } catch (err) {
    // Unreadable is not the same as absent: show the default, but leave
    // whatever is stored alone rather than overwrite it.
    notify("workspace.unreadable", { error: String(err) });
    if (label === MAIN_WINDOW) applyLayout(DEFAULT_LAYOUT);
    return;
  }

  let loaded = false;
  rebuild(() => (loaded = loadSaved(api, saved)));
  if (!loaded) {
    if (label === MAIN_WINDOW) {
      applyLayout(DEFAULT_LAYOUT);
    } else if (saved) {
      addScreen("watchlist");
    } else {
      getCurrentWindow().close();
      return;
    }
  }
  saver = debouncedSetter(layoutKey(label), SAVE_DELAY_MS, (message) => {
    notify("workspace.notSaved", { error: message });
  });
  retitlePanels();
  persist();
}

/** Loads a saved layout or a pop-out seed; false if there is nothing usable. */
function loadSaved(api, saved) {
  if (saved?.version !== LAYOUT_VERSION) return false;
  try {
    if (saved.dockview) {
      api.fromJSON(saved.dockview);
      return true;
    }
    if (Array.isArray(saved.seed)) {
      for (const panel of saved.seed) addScreen(panel.component, { title: panel.title, params: panel.params });
      return true;
    }
  } catch (err) {
    api.clear();
    notify("workspace.unrestorable", { error: String(err) });
  }
  return false;
}

/** Moves a panel into a new native window of its own. */
export async function popOut(panel) {
  const label = newWindowLabel();
  const seed = [{ component: panel.view.contentComponent, params: panel.params ?? {} }];
  try {
    await setSetting(layoutKey(label), { version: LAYOUT_VERSION, seed });
    await invoke("open_window", { label });
    workspace.api?.removePanel(panel);
  } catch (err) {
    notify("workspace.windowFailed", { error: String(err) });
    // The seed was for a window that never opened; nothing else removes it.
    removeSetting(layoutKey(label)).catch(() => {});
  }
}

/**
 * Moves a panel to the next link group (A → B → C → none). Unlinking keeps
 * the symbol it was showing, so the panel does not jump.
 */
export function cycleLink(panel) {
  const current = panel.params?.link;
  const next = nextGroup(current);
  const update = { link: next };
  if (!isLinked(next) && isLinked(current) && links.symbols[current]) {
    update.symbol = links.symbols[current];
  }
  panel.api.updateParameters(update);
  persist();
}

export function screenOf(panel) {
  return panel ? getScreen(panel.view.contentComponent) : undefined;
}
