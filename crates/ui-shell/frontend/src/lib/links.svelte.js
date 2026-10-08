// One shared symbol that linked panels follow (ADR 0013). A panel is linked
// (`params.linked`) or not. Linked, it shows the shared symbol and sets it
// when the trader picks another. Not linked, it keeps its own
// `params.symbol`.
//
// The symbol travels between windows as a Tauri event and is saved, so a
// restart comes back on it. A window that opens asks the others for
// theirs, since one may have moved on since the last save.

import { emit, listen } from "@tauri-apps/api/event";
import { debouncedSetter, getSetting } from "./settings.js";

const EVENT = "link-symbol";
const ASK = "link-symbol-ask";
const SETTING = "workspace.symbol";
/** Where link groups (before ADR 0013) kept their symbols; group A's carries over. */
const OLD_SETTING = "workspace.links";
const SAVE_DELAY_MS = 500;
/** Ignores this window's own events, which Tauri delivers back late. */
const SOURCE = crypto.randomUUID();

class Link {
  /** The shared symbol, or null before anything has been picked. */
  symbol = $state(null);
}

export const link = new Link();

const save = debouncedSetter(SETTING, SAVE_DELAY_MS);

/** Writes a pending change now; see the shell's quit sequence. */
export function flushLinks() {
  return save.flush();
}

export const isLinked = (params) => params?.linked === true;

/** The symbol a panel shows: the shared one when linked, its own when not, else `fallback`. */
export function symbolOf(params, fallback) {
  const own = isLinked(params) ? link.symbol : params?.symbol;
  return own ?? fallback ?? null;
}

/** Sets the shared symbol here, in every other window, and in the store. */
export function setSharedSymbol(symbol) {
  if (!symbol || link.symbol === symbol) return;
  link.symbol = symbol;
  save(symbol);
  emit(EVENT, { source: SOURCE, symbol });
}

/** A panel's pick: the shared symbol when it is linked, its own parameter when not. */
export function chooseSymbol(params, panel, symbol) {
  if (isLinked(params)) setSharedSymbol(symbol);
  else panel?.updateParameters({ symbol });
}

/**
 * The parameters that link or unlink a panel. Unlinking keeps the symbol
 * the panel was showing, so it does not jump.
 */
export function linkPatch(params, linked) {
  if (linked) return { linked: true };
  const shown = symbolOf(params, null);
  return shown ? { linked: false, symbol: shown } : { linked: false };
}

/**
 * A panel's parameters from before ADR 0013, when they named a link group
 * (`link: "a" | "b" | "c" | "none"`): in a group means linked. Parameters
 * without `link` come back as they are.
 */
export function migrateLinkParams(params) {
  if (!params || !("link" in params)) return params;
  const { link: group, ...rest } = params;
  return { ...rest, linked: typeof group === "string" && group !== "none" };
}

let started = null;

/** Restores the saved symbol, then follows and answers other windows; once per window. */
export function startLinks() {
  started ??= (async () => {
    await listen(EVENT, ({ payload }) => {
      if (payload?.source === SOURCE || typeof payload?.symbol !== "string") return;
      link.symbol = payload.symbol;
      save(payload.symbol);
    });
    await listen(ASK, ({ payload }) => {
      if (payload?.source !== SOURCE && link.symbol) emit(EVENT, { source: SOURCE, symbol: link.symbol });
    });
    const saved = (await getSetting(SETTING)) ?? (await getSetting(OLD_SETTING))?.a;
    // An answer from another window may already be in; it is newer.
    if (link.symbol === null && typeof saved === "string") link.symbol = saved;
    await emit(ASK, { source: SOURCE });
  })();
  return started;
}
