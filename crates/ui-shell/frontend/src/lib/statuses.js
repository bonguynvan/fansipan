// Each instrument's own status, as its venue publishes it: halted,
// suspended, or trading under a short-sale restriction or a notice. Risk in
// the engine refuses what a status forbids; these only say so first.

/** `current` (keyed by symbol) with what a batch or snapshot reported. */
export function withStatuses(current, reported) {
  if (!reported?.length) return current;
  return { ...current, ...Object.fromEntries(reported.map((status) => [status.symbol, status])) };
}

/** "halted" or "suspended" for an instrument that does not trade; a status the screens do not know yet counts as halted. */
const stopped = (status) => (status.trading === "suspended" ? "suspended" : "halted");

/** The board's short word for a status, as a catalog id; null while it trades freely. */
export function boardMark(status) {
  if (!status) return null;
  if (status.trading !== "trading") return `status.short.${stopped(status)}`;
  if (status.short_sale_restricted) return "status.short.restricted";
  return null;
}

/**
 * What the ticket says of `symbol`'s status, as `{ id, vars }`, or null when
 * there is nothing to say. A stopped instrument comes first, then a
 * short-sale restriction, then a notice the venue trades it under.
 */
export function statusNotice(status, symbol) {
  if (!status) return null;
  const reason = status.reason ?? "";
  if (status.trading !== "trading") {
    const queued = status.takes_orders ? "Queued" : "";
    const why = reason ? "Why" : "";
    return { id: `ticket.${stopped(status)}${queued}${why}`, vars: { symbol, reason } };
  }
  if (status.short_sale_restricted) return { id: "ticket.shortRestricted", vars: { symbol } };
  if (reason) return { id: "ticket.statusReason", vars: { symbol, reason } };
  return null;
}

/** Every catalog id `statusNotice` and `boardMark` can give. */
export const STATUS_WORDS = Object.freeze([
  "status.short.halted",
  "status.short.suspended",
  "status.short.restricted",
  ...["halted", "suspended"].flatMap((trading) =>
    ["", "Why", "Queued", "QueuedWhy"].map((rest) => `ticket.${trading}${rest}`),
  ),
  "ticket.shortRestricted",
  "ticket.statusReason",
]);
