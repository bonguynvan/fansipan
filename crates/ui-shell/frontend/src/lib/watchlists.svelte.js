// The trader's named watchlists, shared by every watchlist panel in every
// window and saved in the settings store, per market (see `listsSetting`).
// A change travels to other windows as a Tauri event, the way link
// selections do.

import { emit, listen } from "@tauri-apps/api/event";
import { debouncedSetter, getSetting } from "./settings.js";
import { marketInfo } from "./market.svelte.js";
import { listsSetting, normalizeLists } from "./watchlists.js";

const EVENT = "watchlists-change";
const SAVE_DELAY_MS = 300;

// Tauri also delivers an event to the window that sent it, and late: a
// window's own earlier change could land on top of a newer one. Changes
// carry the sending window's id, and a window ignores its own.
const SOURCE = crypto.randomUUID();

class Watchlists {
  /** `[{ id, name, symbols }]`; see watchlists.js. */
  lists = $state.raw([]);
  /**
   * Whether the saved lists are in. Until then nothing can be edited: a
   * change made on top of lists that never loaded would save over them.
   */
  ready = $state(false);
  /** Why the lists did not load, or the last change was not saved or shared; null when fine. */
  error = $state(null);
}

export const watchlists = new Watchlists();

let save = null;

/** Replaces the lists here, saves them, and sends them to the other windows. Ignored until the lists are in. */
export function setLists(next) {
  if (!watchlists.ready || !save) return;
  watchlists.lists = next;
  watchlists.error = null;
  save(next);
  emit(EVENT, { source: SOURCE, lists: next }).catch((err) => (watchlists.error = String(err)));
}

/** Writes a pending change now; see the shell's quit sequence. */
export function flushWatchlists() {
  return save ? save.flush() : Promise.resolve();
}

let started = null;

/**
 * Loads this market's lists and follows other windows' changes; once per
 * window. A change from another window while the load is in flight is
 * newer than what the store held, so it wins.
 */
export function startWatchlists() {
  started ??= (async () => {
    const info = await marketInfo();
    if (!info) throw new Error("the market is not known, so its watchlists cannot be found");
    const setting = listsSetting(info.id);
    let remote = null;
    await listen(EVENT, ({ payload }) => {
      if (payload?.source === SOURCE) return;
      const lists = normalizeLists(payload?.lists);
      if (watchlists.ready) watchlists.lists = lists;
      else remote = lists;
    });
    const saved = normalizeLists(await getSetting(setting));
    watchlists.lists = remote ?? saved;
    save = debouncedSetter(setting, SAVE_DELAY_MS, (message) => (watchlists.error = message));
    watchlists.ready = true;
  })();
  return started;
}
