// Link groups: panels in the same group follow the same symbol. Pick a row
// in a watchlist linked to group A and every chart and ticket on A follows,
// in every window. A panel set to "none" keeps the symbol it was showing.
//
// Selections travel between windows as a Tauri event and are saved, so a
// restart comes back on the same symbols.

import { emit, listen } from "@tauri-apps/api/event";
import { debouncedSetter, getSetting } from "./settings.js";

export const UNLINKED = "none";

/** The groups, in the order a panel's link button cycles through them. */
export const LINK_GROUPS = [
  { id: "a", label: "A", color: "var(--link-a)" },
  { id: "b", label: "B", color: "var(--link-b)" },
  { id: "c", label: "C", color: "var(--link-c)" },
];

const EVENT = "link-select";
const SETTING = "workspace.links";
const SAVE_DELAY_MS = 500;

class Links {
  symbols = $state(Object.fromEntries(LINK_GROUPS.map((g) => [g.id, null])));
}

export const links = new Links();

const save = debouncedSetter(SETTING, SAVE_DELAY_MS);

/** Writes a pending selection now; see the shell's quit sequence. */
export function flushLinks() {
  return save.flush();
}

export function isLinked(group) {
  return LINK_GROUPS.some((g) => g.id === group);
}

/** The group after `group` in the cycle A → B → C → none → A. */
export function nextGroup(group) {
  const index = LINK_GROUPS.findIndex((g) => g.id === group);
  if (index === -1) return LINK_GROUPS[0].id;
  return index === LINK_GROUPS.length - 1 ? UNLINKED : LINK_GROUPS[index + 1].id;
}

export function groupColor(group) {
  return LINK_GROUPS.find((g) => g.id === group)?.color ?? "var(--text-dim)";
}

/**
 * The symbol a panel shows: its group's symbol when linked, its own
 * `params.symbol` when not, else `fallback` (the first listed symbol).
 */
export function symbolOf(params, fallback) {
  const own = isLinked(params?.link) ? links.symbols[params.link] : params?.symbol;
  return own ?? fallback ?? null;
}

/** Sets a group's symbol here and in every other window. */
export function selectSymbol(group, symbol) {
  if (!isLinked(group) || links.symbols[group] === symbol) return;
  links.symbols[group] = symbol;
  save($state.snapshot(links.symbols));
  emit(EVENT, { group, symbol });
}

let started = null;

/** Restores saved selections and follows other windows; once per window. */
export function startLinks() {
  started ??= (async () => {
    await listen(EVENT, ({ payload }) => {
      if (isLinked(payload?.group)) links.symbols[payload.group] = payload.symbol ?? null;
    });
    const saved = await getSetting(SETTING);
    for (const group of LINK_GROUPS) {
      if (typeof saved?.[group.id] === "string") links.symbols[group.id] = saved[group.id];
    }
  })();
  return started;
}
