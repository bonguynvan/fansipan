// The terminal's workspace: its desktops and pages (pages.js), the page
// dockview shows now, and whether each panel follows the shared symbol.
// The whole workspace is saved in the shell's settings store.
//
// dockview holds only the active page. Before the page changes, its
// arrangement is read back into the workspace (`capture`), so a page keeps
// its panels while another is on screen.

import { getLayout, getScreen } from "./registry.js";
import { DEFAULT_LAYOUT } from "./builtins.js";
import { debouncedSetter, getSetting, removeSetting } from "../lib/settings.js";
import { isLinked, linkPatch, migrateLinkParams } from "../lib/links.svelte.js";
import { marketInfo } from "../lib/market.svelte.js";
import { marketPreset } from "../lib/markets.js";
import { panelOrder, stepFrom } from "./panelOrder.js";
import { registerAction } from "../lib/hotkeys.svelte.js";
import { px } from "../lib/ui.svelte.js";
import * as pages from "./pages.js";
import { dockedLayout } from "./docked.js";
import { t } from "../i18n/i18n.svelte.js";

/** Where the workspace is saved: the key from when each window had its own. */
const LAYOUT_KEY = "workspace.layout.main";
const SAVE_DELAY_MS = 400;
/** Where versions with extra windows listed them; each kept its layout under its own key. */
const EXTRA_WINDOWS_KEY = "workspace.windows";
const EXTRA_WINDOW_LABEL = /^ws-[a-z0-9]{1,24}$/;

/** Default names in the trader's language. */
const names = {
  desktop: (n) => t("workspace.desktopName", { n }),
  page: (n) => t("workspace.pageName", { n }),
};

class WorkspaceState {
  api = $state.raw(null);
  /** The desktops and pages (pages.js); null until restored. */
  pages = $state.raw(null);
  /** `{ id, vars, count }` of a message for the notice bar, or null. */
  notice = $state(null);
  /** True while a whole page is being replaced or restored: nothing is saved midway. */
  rebuilding = false;
}

export const workspace = new WorkspaceState();

let saver = null;

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

/** Saves the workspace soon; safe to call on every change. */
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

/** Saved dockview JSON brought up to date: every panel docked, its parameters current. */
function migratedDockview(saved) {
  const dockview = dockedLayout(saved);
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

/** Shows the open page `step` away from the one on screen, wrapping round. */
export function stepPage(step) {
  if (!workspace.pages) return;
  const open = pages.openPages(workspace.pages).map((page) => page.id);
  const next = stepFrom(open, pages.activePage(workspace.pages).id, step);
  if (next) goToPage(next);
}

// The keyboard -----------------------------------------------------------------

/** What the keyboard lands on in a panel it moves to, unless a screen marks its own with `data-autofocus`. */
const FOCUSABLE = 'input:not([disabled]), select:not([disabled]), textarea:not([disabled]), button:not([disabled]), [tabindex="0"]';

/**
 * Moves to the panel `step` away in walking order (panelOrder.js): shows
 * it, and puts the keyboard in it.
 */
export function stepPanel(step) {
  const api = workspace.api;
  if (!api) return;
  const order = panelOrder(
    api.groups.map((group) => {
      const box = group.element.getBoundingClientRect();
      return {
        top: Math.round(box.top),
        left: Math.round(box.left),
        panels: group.panels,
      };
    }),
  );
  const next = stepFrom(order, api.activePanel, step);
  if (!next) return;
  next.api.setActive();
  focusInto(next, FOCUS_TRIES);
}

/** How many frames a panel just shown gets to draw something to focus. */
const FOCUS_TRIES = 4;

/**
 * The keyboard into `panel`: the element its screen marks with
 * `data-autofocus`, else its grid, else the first control. A panel in a
 * background tab is attached only once shown, and its screen may draw a
 * frame or two later, so this waits a few frames for one. With none, the
 * panel's content itself (dockview makes it focusable), so keys stop going
 * to the panel left behind.
 */
function focusInto(panel, tries) {
  requestAnimationFrame(() => {
    const content = panel.group?.element.querySelector(".dv-content-container");
    if (!content) return;
    const target =
      content.querySelector("[data-autofocus]") ??
      content.querySelector('[role="grid"]') ??
      content.querySelector(FOCUSABLE);
    if (target) target.focus();
    else if (tries > 1) focusInto(panel, tries - 1);
    else content.focus();
  });
}

/**
 * Moving around the workspace from the keyboard, registered as the trading
 * actions are, so a trader can rebind them. The defaults are Windows':
 * Ctrl+F6 walks the panels as it walks an app's windows, Ctrl+Tab the
 * pages as it walks tabs. Keys a focused grid uses (arrows, the page keys)
 * would never reach these.
 */
export function registerWorkspaceActions() {
  registerAction({ id: "workspace.next-panel", title: "hotkey.action.nextPanel", defaultKey: "Ctrl+F6", run: () => stepPanel(1) });
  registerAction({
    id: "workspace.previous-panel",
    title: "hotkey.action.previousPanel",
    defaultKey: "Ctrl+Shift+F6",
    run: () => stepPanel(-1),
  });
  registerAction({ id: "workspace.next-page", title: "hotkey.action.nextPage", defaultKey: "Ctrl+Tab", run: () => stepPage(1) });
  registerAction({
    id: "workspace.previous-page",
    title: "hotkey.action.previousPage",
    defaultKey: "Ctrl+Shift+Tab",
    run: () => stepPage(-1),
  });
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

/** The layout a fresh workspace opens with: the market preset's, else the default. */
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

/** A fresh workspace: one page built from the default layout. */
async function startFresh(api) {
  const layout = await defaultLayout();
  // The window may have torn this dockview down while waiting.
  if (workspace.api !== api) return;
  workspace.pages = pages.seedWorkspace(names, { pageName: layout ? t(layout.title) : null });
  showActivePage(layout?.id ?? null);
}

/**
 * The panels of the extra windows an older version kept, and the keys that
 * held them. A window whose layout cannot be read gives none.
 */
async function extraWindows() {
  const listed = await getSetting(EXTRA_WINDOWS_KEY).catch(() => null);
  if (!Array.isArray(listed)) return { panels: [], keys: [] };
  const labels = listed.filter((label) => typeof label === "string" && EXTRA_WINDOW_LABEL.test(label));
  const panels = [];
  for (const label of labels) {
    const saved = await getSetting(`workspace.layout.${label}`).catch(() => null);
    const kept = pages.reviveWorkspace(saved, names) ?? pages.fromSingleLayout(saved, names, names.page(1));
    if (kept) panels.push(...pages.panelsOf(kept));
  }
  return { panels, keys: [EXTRA_WINDOWS_KEY, ...labels.map((label) => `workspace.layout.${label}`)] };
}

/** Drops the extra windows' keys once the workspace holding their panels is saved. */
async function forgetExtraWindows({ panels, keys }) {
  await flushLayout();
  await Promise.all(keys.map((key) => removeSetting(key).catch(() => {})));
  if (panels.length) workspace.notice = { id: "workspace.windowsMerged", count: panels.length };
}

/**
 * Restores the workspace from its save: a workspace of pages, or a single
 * layout from before pages (migrated into the first page). Panels an older
 * version kept in extra windows join the page on screen. With nothing
 * usable, it starts fresh.
 */
export async function restore(api) {
  workspace.api = api;
  workspace.pages = null;
  // Saving stays off until the save is in: a resize or an empty first page
  // must never overwrite it while it is still loading.
  saver = null;

  let saved;
  try {
    saved = await getSetting(LAYOUT_KEY);
  } catch (err) {
    // Unreadable is not the same as absent: show the default, but leave
    // whatever is stored alone rather than overwrite it.
    notify("workspace.unreadable", { error: String(err) });
    await startFresh(api);
    return;
  }
  if (workspace.api !== api) return;

  const restored = pages.reviveWorkspace(saved, names) ?? pages.fromSingleLayout(saved, names, names.page(1));
  if (!restored && saved !== null && saved !== undefined) {
    // Something is saved that this version cannot read, perhaps from a newer
    // one. Keep it: show a default, and save nothing this session.
    notify("workspace.unknownSave");
    await startFresh(api);
    return;
  }
  saver = debouncedSetter(LAYOUT_KEY, SAVE_DELAY_MS, (message) => {
    notify("workspace.notSaved", { error: message });
  });
  if (restored) {
    const extra = await extraWindows();
    if (workspace.api !== api) return;
    workspace.pages = pages.addIncoming(restored, extra.panels);
    showActivePage();
    if (extra.keys.length) {
      forgetExtraWindows(extra).catch((err) => notify("workspace.notSaved", { error: String(err) }));
    }
  } else {
    await startFresh(api);
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
