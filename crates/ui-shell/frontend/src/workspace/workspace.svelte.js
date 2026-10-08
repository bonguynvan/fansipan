// One native window's workspace: its desktops and pages (pages.js), the
// page dockview shows now, and whether each panel follows the shared
// symbol (ADR 0013). The whole workspace is saved per window in the
// shell's settings store; the shell reopens extra windows at start and
// each window restores itself from its key.
//
// dockview holds only the active page. Before the page changes, its
// arrangement is read back into the workspace (`capture`), so a page keeps
// its panels while another is on screen.

import { invoke } from "@tauri-apps/api/core";
import { emit, listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { getLayout, getScreen } from "./registry.js";
import { DEFAULT_LAYOUT } from "./builtins.js";
import { debouncedSetter, getSetting, removeSetting, setSetting } from "../lib/settings.js";
import { isLinked, linkPatch, migrateLinkParams } from "../lib/links.svelte.js";
import { marketInfo } from "../lib/market.svelte.js";
import { marketPreset } from "../lib/markets.js";
import { newWindowLabel } from "./labels.js";
import { px } from "../lib/ui.svelte.js";
import * as pages from "./pages.js";
import { t } from "../i18n/i18n.svelte.js";

export const MAIN_WINDOW = "main";
const SAVE_DELAY_MS = 400;
/** An extra window sends a panel back to the main window's page on this event. */
const DOCK_EVENT = "workspace-dock";
/** Where the first panel lifted off the dock floats, and how big, in pixels at 100%; the next ones cascade. */
const FLOATING_BOX = { x: 80, y: 60, width: 460, height: 340 };
const FLOATING_STEP = 28;
const FLOATING_SPAN = 8;

/** Default names in the trader's language. */
const names = {
  desktop: (n) => t("workspace.desktopName", { n }),
  page: (n) => t("workspace.pageName", { n }),
};

class WorkspaceState {
  api = $state.raw(null);
  label = $state(MAIN_WINDOW);
  /** This window's desktops and pages (pages.js); null until restored. */
  pages = $state.raw(null);
  /** `{ id, vars, count }` of a message for the notice bar, or null. */
  notice = $state(null);
  /**
   * True while a whole page is being replaced or restored. Panels come and
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

/**
 * A page whose save could not be restored. It shows empty, but its save is
 * kept as it was until the trader puts a panel on it: a save from another
 * version must survive a launch of this one.
 */
let heldPageId = null;

/** Reads the page on screen back into the workspace. */
function capture() {
  if (!workspace.api || !workspace.pages) return;
  if (pages.activePage(workspace.pages).id === heldPageId) {
    if (workspace.api.totalPanels === 0) return;
    heldPageId = null;
  }
  workspace.pages = pages.withPageLayout(workspace.pages, workspace.api.toJSON());
}

/** Saves this window's workspace soon; safe to call on every change. */
export function persist() {
  if (!saver || !workspace.api || !workspace.pages || workspace.rebuilding) return;
  capture();
  saver(workspace.pages);
}

/** Writes a pending save now; see the shell's quit sequence. */
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
 * Adds a panel showing a registered screen to the page on screen. An
 * unregistered id still gets a panel, which explains what is missing
 * instead of failing the whole page. `options.title` (a message id or plain
 * text) overrides the screen's title and is kept in the panel's `titleKey`
 * parameter, so the tab can be re-labelled in another language.
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
 * Arranges a registered layout on the (empty) page on screen. Size hints
 * (`initialWidth`, `initialHeight`) are applied once every panel is in:
 * dockview splits evenly while a layout is still being built.
 */
function buildLayout(layout) {
  const sized = [];
  layout.build((screenId, options = {}) => {
    const { initialWidth, initialHeight, ...rest } = options;
    const panel = addScreen(screenId, rest);
    // Size hints are in pixels at 100%, like every size in a layout.
    const size = { ...(initialWidth && { width: px(initialWidth) }), ...(initialHeight && { height: px(initialHeight) }) };
    if (panel && Object.keys(size).length) sized.push([panel, size]);
    return panel;
  });
  for (const [panel, size] of sized) panel.group.api.setSize(size);
}

/** Saved dockview JSON with every panel's parameters brought up to ADR 0013. */
function migratedDockview(dockview) {
  const panels = Object.fromEntries(
    Object.entries(dockview?.panels ?? {}).map(([id, panel]) => [
      id,
      panel?.params ? { ...panel, params: migrateLinkParams(panel.params) } : panel,
    ]),
  );
  return { ...dockview, panels };
}

/**
 * Puts the active page on screen: its saved arrangement, then any panels
 * moved to it since it was last shown, then `layoutId`'s panels for a new
 * page from a template.
 */
function showActivePage(layoutId = null) {
  const api = workspace.api;
  if (!api || !workspace.pages) return;
  rebuild(() => {
    api.clear();
    const page = pages.activePage(workspace.pages);
    if (page.dockview) {
      try {
        api.fromJSON(migratedDockview(page.dockview));
      } catch (err) {
        api.clear();
        heldPageId = page.id;
        notify("workspace.unrestorable", { name: page.name, error: String(err) });
      }
    }
    const [rest, incoming] = pages.takeIncoming(workspace.pages);
    workspace.pages = rest;
    for (const panel of incoming) {
      addScreen(panel.component, { title: panel.title, params: migrateLinkParams(panel.params) });
    }
    const layout = layoutId ? getLayout(layoutId) : null;
    if (layout) {
      // A plugin's layout can throw; the page keeps whatever it built.
      try {
        buildLayout(layout);
      } catch (err) {
        notify("workspace.layoutFailed", { name: t(layout.title), error: String(err) });
      }
    }
  });
  retitlePanels();
  persist();
  dockWaiting();
}

/** Applies `change` to the workspace, then shows the active page if it is another one. */
function change(apply) {
  if (!workspace.pages) return;
  capture();
  const before = pages.activePage(workspace.pages).id;
  workspace.pages = apply(workspace.pages);
  if (pages.activePage(workspace.pages).id !== before) showActivePage();
  else persist();
}

// Pages ----------------------------------------------------------------------

export function goToPage(id) {
  change((ws) => pages.switchPage(ws, id));
}

/** A new page: blank, or built from a registered layout (a template). */
export function newPage(layoutId = null) {
  if (!workspace.pages) return;
  capture();
  const layout = layoutId ? getLayout(layoutId) : null;
  const name = layout ? t(layout.title) : names.page(pages.activeDesktop(workspace.pages).pages.length + 1);
  workspace.pages = pages.addPage(workspace.pages, name);
  showActivePage(layout?.id ?? null);
}

/** A new page holding a copy of the page on screen. */
export function copyPage() {
  if (!workspace.pages) return;
  capture();
  const page = pages.activePage(workspace.pages);
  workspace.pages = pages.addPage(workspace.pages, t("workspace.copyOf", { name: page.name }), page.dockview);
  showActivePage();
}

/** Closes a tab; the page stays in the saved list. */
export function closePageTab(id) {
  change((ws) => pages.closePage(ws, id, names));
}

export function reopenPage(id) {
  change((ws) => pages.reopenPage(ws, id));
}

export function renamePage(id, name) {
  change((ws) => pages.renamePage(ws, id, name));
}

/** Deletes a saved page for good. */
export function deletePage(id) {
  change((ws) => pages.deletePage(ws, id, names));
}

/** What a panel is, apart from where it sits: enough to show it again elsewhere. */
function specOf(panel) {
  return { component: panel.view.contentComponent, params: panel.params ?? {}, title: panel.params?.titleKey ?? null };
}

/** Moves a panel off the page on screen to another page of this desktop. */
export function movePanelToPage(panel, pageId) {
  if (!workspace.pages || !workspace.api) return;
  capture();
  workspace.pages = pages.movePanelToPage(workspace.pages, pageId, specOf(panel));
  workspace.api.removePanel(panel);
  persist();
}

/** The open pages of this desktop other than the one on screen: where a panel can move. */
export function otherPages() {
  if (!workspace.pages) return [];
  const here = pages.activePage(workspace.pages).id;
  return pages.openPages(workspace.pages).filter((p) => p.id !== here);
}

// Floating and docking -------------------------------------------------------

export const isFloating = (group) => group?.api.location.type === "floating";

/** Lifts a group off the dock to float over the page, leaving the dock as it was. */
export function floatGroup(group) {
  const api = workspace.api;
  if (!api || group?.api.location.type !== "grid") return;
  const n = api.groups.filter(isFloating).length % FLOATING_SPAN;
  api.addFloatingGroup(group, {
    x: px(FLOATING_BOX.x + n * FLOATING_STEP),
    y: px(FLOATING_BOX.y + n * FLOATING_STEP),
    width: px(FLOATING_BOX.width),
    height: px(FLOATING_BOX.height),
  });
  persist();
}

/** Puts a floating group back into the dock, at the right. */
export function dockGroup(group) {
  if (!isFloating(group)) return;
  group.api.moveTo({ position: "right" });
  persist();
}

/**
 * Sends a panel from an extra window back to the main window, which docks
 * it into its page on screen. The panel leaves this window only once the
 * main window has been sent it.
 */
export async function dockInMain(panel) {
  try {
    await emit(DOCK_EVENT, specOf(panel));
    workspace.api?.removePanel(panel);
  } catch (err) {
    notify("workspace.dockFailed", { error: String(err) });
  }
}

let docking = null;
/** Panels sent back before this window's page was on screen; docked once it is. */
const dockQueue = [];

function dockWaiting() {
  while (dockQueue.length && workspace.pages && !workspace.rebuilding) {
    const panel = dockQueue.shift();
    addScreen(panel.component, { title: panel.title ?? undefined, params: migrateLinkParams(panel.params) });
  }
  persist();
}

/**
 * The main window takes panels sent back from extra windows; once. One
 * that arrives while the main window is still restoring waits for its page,
 * which would otherwise clear it away.
 */
function takeDockedPanels() {
  docking ??= listen(DOCK_EVENT, ({ payload }) => {
    if (typeof payload?.component !== "string" || !payload.component) return;
    const params = payload.params && typeof payload.params === "object" ? payload.params : {};
    dockQueue.push({ component: payload.component, params, title: payload.title ?? null });
    dockWaiting();
  });
  return docking;
}

// Desktops ---------------------------------------------------------------------

export function newDesktop() {
  change((ws) => pages.addDesktop(ws, names));
}

export function goToDesktop(id) {
  change((ws) => pages.switchDesktop(ws, id));
}

export function renameDesktop(id, name) {
  change((ws) => pages.renameDesktop(ws, id, name));
}

export function removeDesktop(id) {
  change((ws) => pages.removeDesktop(ws, id));
}

// Restoring --------------------------------------------------------------------

/** The layout a fresh main window opens with: the market preset's, else the default. */
async function defaultLayout() {
  const info = await marketInfo();
  return getLayout(marketPreset(info?.id).layout ?? DEFAULT_LAYOUT) ?? getLayout(DEFAULT_LAYOUT);
}

/**
 * Labels every tab in the current language. A saved page holds tab text
 * in whatever language it was saved in; the source of each title is the
 * panel's `titleKey` or its screen's title.
 */
export function retitlePanels() {
  for (const panel of workspace.api?.panels ?? []) {
    const key = panel.params?.titleKey ?? screenOf(panel)?.title;
    if (key) panel.api.setTitle(t(key));
  }
}

/** A fresh main window: one page built from the default layout. */
async function startFresh(api) {
  const layout = await defaultLayout();
  // The window may have torn this dockview down while waiting.
  if (workspace.api !== api) return;
  workspace.pages = pages.seedWorkspace(names, { pageName: layout ? t(layout.title) : null });
  showActivePage(layout?.id ?? null);
}

/**
 * Restores this window from its save: a workspace of pages, a single
 * layout from before pages (migrated into the first page), or a pop-out's
 * panel. With nothing usable, the main window starts fresh; an extra
 * window shows a watchlist if its save was damaged, and closes if it never
 * had one (it would otherwise reopen, empty, forever).
 */
export async function restore(api, label) {
  workspace.api = api;
  workspace.label = label;
  workspace.pages = null;
  if (label === MAIN_WINDOW) takeDockedPanels().catch((err) => notify("workspace.dockFailed", { error: String(err) }));
  // Saving stays off until the save is in: a resize or an empty first page
  // must never overwrite it while it is still loading.
  saver = null;

  let saved;
  try {
    saved = await getSetting(layoutKey(label));
  } catch (err) {
    // Unreadable is not the same as absent: show the default, but leave
    // whatever is stored alone rather than overwrite it.
    notify("workspace.unreadable", { error: String(err) });
    if (label === MAIN_WINDOW) await startFresh(api);
    return;
  }
  if (workspace.api !== api) return;

  const restored = pages.reviveWorkspace(saved, names) ?? pages.fromSingleLayout(saved, names, names.page(1));
  if (!restored && saved !== null && saved !== undefined) {
    // Something is saved that this version cannot read, perhaps from a newer
    // one. Keep it: show a default, and save nothing this session.
    notify("workspace.unknownSave");
    if (label === MAIN_WINDOW) await startFresh(api);
    else {
      workspace.pages = pages.seedWorkspace(names, { panels: [{ component: "watchlist", params: {}, title: null }] });
      showActivePage();
    }
    return;
  }
  saver = debouncedSetter(layoutKey(label), SAVE_DELAY_MS, (message) => {
    notify("workspace.notSaved", { error: message });
  });
  if (restored) {
    workspace.pages = restored;
    showActivePage();
  } else if (label === MAIN_WINDOW) {
    await startFresh(api);
  } else {
    getCurrentWindow().close();
  }
}

/** Moves a panel into a new native window of its own. */
export async function popOut(panel) {
  const label = newWindowLabel();
  const seed = pages.seedWorkspace(names, {
    panels: [{ component: panel.view.contentComponent, params: panel.params ?? {}, title: panel.params?.titleKey ?? null }],
  });
  try {
    await setSetting(layoutKey(label), seed);
    await invoke("open_window", { label });
    workspace.api?.removePanel(panel);
  } catch (err) {
    notify("workspace.windowFailed", { error: String(err) });
    // The seed was for a window that never opened; nothing else removes it.
    removeSetting(layoutKey(label)).catch(() => {});
  }
}

/** Links a panel to the shared symbol, or unlinks it keeping what it shows. */
export function toggleLink(panel) {
  panel.api.updateParameters(linkPatch(panel.params, !isLinked(panel.params)));
  persist();
}

/** Sets an unlinked panel's own symbol. */
export function setPanelSymbol(panel, symbol) {
  panel.api.updateParameters({ symbol });
  persist();
}

export function screenOf(panel) {
  return panel ? getScreen(panel.view.contentComponent) : undefined;
}
