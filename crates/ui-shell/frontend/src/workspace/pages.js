// A native window's pages and desktops as plain data. Desktops
// hold pages. A desktop's open pages are its tabs, its closed ones a saved
// list to reopen. A page keeps dockview's own JSON, or null before it has
// first been shown, and the panels moved to it from another page that it
// has not shown yet (`incoming`). Every change returns a new workspace and
// leaves its input alone; the store (workspace.svelte.js) shows the active
// page in dockview and saves the rest.
//
//   Workspace { version, desktops, activeDesktopId }
//   Desktop   { id, name, pages, activePageId }
//   Page      { id, name, open, dockview, incoming: [{ component, params, title }] }
//
// `names` gives default names in the trader's language: `{ desktop(n), page(n) }`.

export const VERSION = 2;

const newId = (prefix) => `${prefix}-${crypto.randomUUID().slice(0, 8)}`;
const isObject = (value) => value !== null && typeof value === "object" && !Array.isArray(value);

/** A name unique among `taken`: "Board", then "Board 2", "Board 3"… */
export function uniqueName(base, taken) {
  const names = new Set(taken);
  if (!names.has(base)) return base;
  let n = 2;
  while (names.has(`${base} ${n}`)) n += 1;
  return `${base} ${n}`;
}

function blankPage(name, incoming = []) {
  return { id: newId("page"), name, open: true, dockview: null, incoming };
}

function blankDesktop(name, pageName, incoming = []) {
  const page = blankPage(pageName, incoming);
  return { id: newId("desktop"), name, pages: [page], activePageId: page.id };
}

/**
 * A fresh workspace: one desktop with one empty page, named `pageName` or
 * the default, holding `panels` (`[{ component, params, title }]`) to show
 * first.
 */
export function seedWorkspace(names, { pageName = null, panels = [] } = {}) {
  const desktop = blankDesktop(names.desktop(1), pageName ?? names.page(1), panels);
  return { version: VERSION, desktops: [desktop], activeDesktopId: desktop.id };
}

// --- reading ---------------------------------------------------------------

export function activeDesktop(ws) {
  return ws.desktops.find((d) => d.id === ws.activeDesktopId) ?? ws.desktops[0];
}

/** The page dockview shows: always an open one. */
export function activePage(ws) {
  const desktop = activeDesktop(ws);
  return (
    desktop.pages.find((p) => p.id === desktop.activePageId && p.open) ??
    desktop.pages.find((p) => p.open) ??
    desktop.pages[0]
  );
}

export const openPages = (ws) => activeDesktop(ws).pages.filter((p) => p.open);
export const savedPages = (ws) => activeDesktop(ws).pages.filter((p) => !p.open);

// --- changing ----------------------------------------------------------------

function withDesktop(ws, change) {
  const target = activeDesktop(ws);
  return { ...ws, desktops: ws.desktops.map((d) => (d.id === target.id ? change(d) : d)) };
}

function withPage(ws, id, change) {
  return withDesktop(ws, (d) => ({ ...d, pages: d.pages.map((p) => (p.id === id ? change(p) : p)) }));
}

/** Keeps "at least one open page, and the active one is open" for a desktop. */
function keepOneOpen(desktop, names) {
  let pages = desktop.pages;
  if (!pages.some((p) => p.open)) {
    pages = [...pages, blankPage(uniqueName(names.page(pages.length + 1), pages.map((p) => p.name)))];
  }
  const activeOpen = pages.some((p) => p.id === desktop.activePageId && p.open);
  return { ...desktop, pages, activePageId: activeOpen ? desktop.activePageId : pages.find((p) => p.open).id };
}

/** The active page with dockview's current JSON, as it is now on screen. */
export function withPageLayout(ws, dockview) {
  return withPage(ws, activePage(ws).id, (p) => ({ ...p, dockview }));
}

/** The active page's waiting panels, and the workspace without them. */
export function takeIncoming(ws) {
  const page = activePage(ws);
  if (page.incoming.length === 0) return [ws, []];
  return [withPage(ws, page.id, (p) => ({ ...p, incoming: [] })), page.incoming];
}

/** Adds `panels` to the active page, to show the next time it is put on screen. */
export function addIncoming(ws, panels) {
  if (panels.length === 0) return ws;
  return withPage(ws, activePage(ws).id, (p) => ({ ...p, incoming: [...p.incoming, ...panels] }));
}

/**
 * Every panel a workspace holds, on every desktop and page, as
 * `{ component, params, title }`: what it takes to show them elsewhere.
 */
export function panelsOf(ws) {
  return ws.desktops.flatMap((desktop) =>
    desktop.pages.flatMap((page) => [
      ...Object.values(page.dockview?.panels ?? {})
        .map((panel) =>
          revivePanel({ component: panel?.contentComponent, params: panel?.params, title: panel?.params?.titleKey }),
        )
        .filter(Boolean),
      ...page.incoming,
    ]),
  );
}

/** A new open page, made active. `dockview` copies an arrangement ("Copy of this page"). */
export function addPage(ws, name, dockview = null) {
  const desktop = activeDesktop(ws);
  const page = { ...blankPage(uniqueName(name, desktop.pages.map((p) => p.name))), dockview };
  return withDesktop(ws, (d) => ({ ...d, pages: [...d.pages, page], activePageId: page.id }));
}

export function switchPage(ws, id) {
  if (!activeDesktop(ws).pages.some((p) => p.id === id && p.open)) return ws;
  return withDesktop(ws, (d) => ({ ...d, activePageId: id }));
}

/** Closes a tab; the page stays in the saved list. */
export function closePage(ws, id, names) {
  return withDesktop(ws, (d) => keepOneOpen({ ...d, pages: d.pages.map((p) => (p.id === id ? { ...p, open: false } : p)) }, names));
}

export function reopenPage(ws, id) {
  if (!activeDesktop(ws).pages.some((p) => p.id === id)) return ws;
  return withDesktop(ws, (d) => ({
    ...d,
    pages: d.pages.map((p) => (p.id === id ? { ...p, open: true } : p)),
    activePageId: id,
  }));
}

export function renamePage(ws, id, name) {
  const clean = String(name ?? "").trim();
  return clean ? withPage(ws, id, (p) => ({ ...p, name: clean })) : ws;
}

export function deletePage(ws, id, names) {
  return withDesktop(ws, (d) => keepOneOpen({ ...d, pages: d.pages.filter((p) => p.id !== id) }, names));
}

/**
 * Sends a panel (`{ component, params, title }`) to another page of the
 * active desktop, where it appears when that page is next shown.
 */
export function movePanelToPage(ws, pageId, panel) {
  if (pageId === activePage(ws).id || !activeDesktop(ws).pages.some((p) => p.id === pageId)) return ws;
  return withPage(ws, pageId, (p) => ({ ...p, incoming: [...p.incoming, panel] }));
}

export function addDesktop(ws, names) {
  const name = uniqueName(names.desktop(ws.desktops.length + 1), ws.desktops.map((d) => d.name));
  const desktop = blankDesktop(name, names.page(1));
  return { ...ws, desktops: [...ws.desktops, desktop], activeDesktopId: desktop.id };
}

export function switchDesktop(ws, id) {
  return ws.desktops.some((d) => d.id === id) ? { ...ws, activeDesktopId: id } : ws;
}

export function renameDesktop(ws, id, name) {
  const clean = String(name ?? "").trim();
  if (!clean) return ws;
  return { ...ws, desktops: ws.desktops.map((d) => (d.id === id ? { ...d, name: clean } : d)) };
}

/** Removes a desktop and its pages; the last desktop stays. */
export function removeDesktop(ws, id) {
  if (ws.desktops.length <= 1) return ws;
  const desktops = ws.desktops.filter((d) => d.id !== id);
  const activeDesktopId = desktops.some((d) => d.id === ws.activeDesktopId) ? ws.activeDesktopId : desktops[0].id;
  return { ...ws, desktops, activeDesktopId };
}

// --- reading back a save -------------------------------------------------------

function revivePanel(saved) {
  if (!isObject(saved) || typeof saved.component !== "string" || !saved.component) return null;
  return {
    component: saved.component,
    params: isObject(saved.params) ? saved.params : {},
    title: typeof saved.title === "string" ? saved.title : null,
  };
}

function reviveId(saved, prefix, seen) {
  const id = typeof saved === "string" && saved && !seen.has(saved) ? saved : newId(prefix);
  seen.add(id);
  return id;
}

const reviveName = (saved, fallback) => (typeof saved === "string" && saved.trim() ? saved.trim() : fallback);

function reviveDesktop(saved, seen, names, n) {
  if (!isObject(saved)) return null;
  const pages = (Array.isArray(saved.pages) ? saved.pages : []).filter(isObject).map((p, i) => ({
    id: reviveId(p.id, "page", seen),
    name: reviveName(p.name, names.page(i + 1)),
    open: p.open !== false,
    dockview: isObject(p.dockview) ? p.dockview : null,
    incoming: (Array.isArray(p.incoming) ? p.incoming : []).map(revivePanel).filter(Boolean),
  }));
  const desktop = {
    id: reviveId(saved.id, "desktop", seen),
    name: reviveName(saved.name, names.desktop(n)),
    pages,
    activePageId: saved.activePageId,
  };
  return keepOneOpen(desktop, names);
}

/**
 * A saved workspace made safe to show: the store may hold anything, from
 * a hand-edited file to a partial write. Ids are made unique, names filled
 * in, unknown fields dropped, and every desktop given an open page. Null
 * when the save is not a workspace of this version.
 */
export function reviveWorkspace(saved, names) {
  if (!isObject(saved) || saved.version !== VERSION || !Array.isArray(saved.desktops)) return null;
  const seen = new Set();
  const desktops = saved.desktops.map((d, i) => reviveDesktop(d, seen, names, i + 1)).filter(Boolean);
  if (desktops.length === 0) return null;
  const activeDesktopId = desktops.some((d) => d.id === saved.activeDesktopId) ? saved.activeDesktopId : desktops[0].id;
  return { version: VERSION, desktops, activeDesktopId };
}

/**
 * A layout saved before pages and desktops (version 1: dockview's JSON, or
 * the panel list an older version gave a window of its own) as the first
 * page of a fresh workspace, named
 * `pageName`. Null when it holds nothing to show.
 */
export function fromSingleLayout(saved, names, pageName) {
  if (!isObject(saved) || saved.version !== 1) return null;
  if (isObject(saved.dockview)) {
    return withPageLayout(seedWorkspace(names, { pageName }), saved.dockview);
  }
  const panels = (Array.isArray(saved.seed) ? saved.seed : []).map(revivePanel).filter(Boolean);
  return panels.length ? seedWorkspace(names, { pageName, panels }) : null;
}
