// A price picked on one screen for the order tickets in its link group: a
// depth ladder row clicked fills the ticket's limit price, and its side
// when the click was on a side of the book. Picks travel between windows
// as symbols do; each carries an id, so a ticket applies it once however
// many times it arrives.

import { emit, listen } from "@tauri-apps/api/event";
import { isLinked } from "./links.svelte.js";

const EVENT = "price-pick";
/** Pick ids remembered, so a late echo cannot bring an older pick back. */
const SEEN_LIMIT = 100;

class Picks {
  /** The latest pick per link group: `{ id, symbol, price, side }`. */
  latest = $state.raw({});
}

export const picks = new Picks();

// Tauri also delivers an event to the window that sent it, and late: a
// window that picked 100 and then 110 would see 100 come back after 110.
// A pick already taken here is never taken again.
const seen = new Set();

function remember(id) {
  seen.add(id);
  if (seen.size > SEEN_LIMIT) seen.delete(seen.values().next().value);
}

/**
 * Hands a price for `symbol` to the tickets of `group`, in every window.
 * `price` is the exact price as a decimal string; `side` is "buy", "sell",
 * or null to leave the ticket's side as it is. An unlinked group has no
 * tickets to fill, so the pick goes nowhere. Resolves once other windows
 * have been sent it; this window's tickets have it already.
 */
export async function pickPrice(group, { symbol, price, side = null }) {
  if (!isLinked(group)) return;
  const pick = { id: crypto.randomUUID(), symbol, price, side };
  remember(pick.id);
  picks.latest = { ...picks.latest, [group]: pick };
  await emit(EVENT, { group, pick });
}

/** The picks after `payload` arrives from a window: unchanged for a malformed payload or a pick `seenIds` holds. */
export function received(latest, payload, seenIds) {
  const pick = payload?.pick;
  if (!isLinked(payload?.group) || !pick?.id || seenIds.has(pick.id)) return latest;
  return { ...latest, [payload.group]: pick };
}

/** The pick a ticket on `group` showing `symbol` should apply now: a new one for its symbol, else null. */
export function pickToApply(latest, group, symbol, appliedId) {
  const pick = isLinked(group) ? latest[group] : null;
  return pick && pick.id !== appliedId && pick.symbol === symbol ? pick : null;
}

let started = null;

/** Follows picks made in other windows; once per window. */
export function startPicks() {
  started ??= listen(EVENT, ({ payload }) => {
    const next = received(picks.latest, payload, seen);
    if (next === picks.latest) return;
    remember(payload.pick.id);
    picks.latest = next;
  });
  return started;
}
