// Each market's phase of the day and the order types it takes now, as the
// adapter publishes them. The ticket offers what an instrument's
// market takes; the board shows each market's phase. Nothing here knows a
// market or an order type by name: the names come with the session.

/** Offered for a market the adapter publishes no session for: what every venue takes. */
export const DEFAULT_ORDER_TYPES = Object.freeze([
  Object.freeze({ kind: "limit", time_in_force: "day", name: null }),
  Object.freeze({ kind: "market", time_in_force: "day", name: null }),
]);

/** `current` (keyed by market) with what a batch or snapshot reported. */
export function withSessions(current, reported) {
  if (!reported?.length) return current;
  return { ...current, ...Object.fromEntries(reported.map((session) => [session.market, session])) };
}

/**
 * The order types `exchange` takes now: its session's, or the defaults for
 * a market with no session. An empty list means it takes no orders now.
 */
export function orderTypesFor(exchange, sessions) {
  const session = exchange ? sessions[exchange] : undefined;
  return session ? session.order_types : DEFAULT_ORDER_TYPES;
}

/** Names an order type in a select. */
export const typeKey = (type) => `${type.kind}:${type.time_in_force}`;

/** Whether an order of this type carries a limit price. */
export const isPriced = (type) => type?.kind === "limit";

/**
 * The type to show selected: the one asked for while the market still
 * takes it, else a limit order for the day (it states its price, so it is
 * never a surprise), else the first offered; null when nothing is.
 */
export function pickOrderType(offered, wantedKey) {
  return (
    offered.find((type) => typeKey(type) === wantedKey) ??
    offered.find((type) => isPriced(type) && type.time_in_force === "day") ??
    offered[0] ??
    null
  );
}

/**
 * `names` with every name a reported session gives its order types, keyed
 * by market and type. Names stay after the session moves on, so an order
 * keeps the name it was placed under.
 */
export function withTypeNames(names, reported) {
  const seen = (reported ?? []).flatMap((session) =>
    session.order_types.filter((type) => type.name).map((type) => [`${session.market}|${typeKey(type)}`, type.name]),
  );
  return seen.length ? { ...names, ...Object.fromEntries(seen) } : names;
}

/** The venue's name for an order's type on `exchange`, once a session has named it; null before. */
export function venueNameOf(order, exchange, names) {
  return names[`${exchange}|${typeKey(order)}`] ?? null;
}
