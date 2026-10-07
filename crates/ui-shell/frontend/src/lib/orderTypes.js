// Order types and session phases in words. A venue's own name ("ATO",
// "MTL") wins: it is what the venue's traders know. Without one, the
// generic kind and time in force are said in the reader's language.

const KINDS = ["limit", "market", "marketToLimit"];
const TIMES_IN_FORCE = ["day", "ioc", "fok", "atOpen", "atClose", "afterClose"];
const PHASES = ["preOpen", "openingAuction", "continuous", "break", "closingAuction", "afterClose", "closed"];

const known = (value, values) => (values.includes(value) ? value : "unknown");

/** `type` (`{ kind, time_in_force, name? }`) in words, with `t` translating. */
export function orderTypeName(type, t) {
  if (type.name) return type.name;
  const kind = t(`orderKind.${known(type.kind, KINDS)}`);
  const timeInForce = known(type.time_in_force, TIMES_IN_FORCE);
  return timeInForce === "day" ? kind : `${kind} · ${t(`timeInForce.${timeInForce}`)}`;
}

/** A session's phase in words: the venue's name for it, or the generic phase. */
export function phaseName(session, t) {
  return session.name ?? t(`session.${known(session.phase, PHASES)}`);
}
