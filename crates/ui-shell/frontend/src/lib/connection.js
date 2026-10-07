// The venue's connections (market data, orders) as the shell reports them,
// summed up for the status badge in the top bar.

/** How much each state needs the trader's attention. */
const SEVERITY = { up: 0, degraded: 1, unknown: 2, reconnecting: 3, down: 4 };
const NAMES = ["marketData", "orders"];

const stateOf = (state) => (state in SEVERITY ? state : "unknown");
const nameOf = (connection) => (NAMES.includes(connection) ? connection : "unknown");
const rank = (name) => (NAMES.includes(name) ? NAMES.indexOf(name) : NAMES.length);

/** `current` (keyed by connection) with what a batch or snapshot reported. */
export function withConnections(current, reported) {
  if (!reported?.length) return current;
  return { ...current, ...Object.fromEntries(reported.map((c) => [c.connection, c])) };
}

/** Each connection with a known name and state, market data first. */
export function connectionLines(connections) {
  return Object.values(connections)
    .map((c) => ({ connection: nameOf(c.connection), state: stateOf(c.state), detail: c.detail ?? null }))
    .sort((a, b) => rank(a.connection) - rank(b.connection));
}

/**
 * What the badge shows: `state` "up" when every connection is (or none has
 * been reported), otherwise the worst state, and `connection` naming the
 * one in it, or null when all of them are.
 */
export function summarize(connections) {
  const lines = connectionLines(connections);
  const worst = lines.reduce((w, c) => (SEVERITY[c.state] > SEVERITY[w.state] ? c : w), {
    state: "up",
    connection: null,
  });
  if (worst.state === "up") return { state: "up", connection: null };
  const alike = lines.filter((c) => c.state === worst.state).length;
  return { state: worst.state, connection: alike === lines.length ? null : worst.connection };
}
