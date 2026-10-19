// The ticket's questions to core about sizing from risk (ADR 0021) and the
// exits it sends with an order (ADR 0022). Core works the size out in exact
// decimals, costs and caps included, and checks the exits; this decides
// what to ask and send, how the answer reads, and what the trader keeps.

/** Where the trader's budget is kept, for every ticket. */
export const RISK_SETTING = "screen.order-ticket.risk";

const positive = (value) => value !== null && value !== undefined && value !== "" && Number(value) > 0;

/** A number as plain decimal text: the shell parses no exponents. */
export function plain(value) {
  const text = String(value);
  if (!/e/i.test(text)) return text;
  return Number(value)
    .toFixed(20)
    .replace(/\.?0+$/, "");
}

/**
 * The arguments for the shell's `size_for_risk`, or null until there is a
 * symbol, a type, an entry, a stop, and a budget to ask about.
 * @param {{ symbol: string | null, side: string, orderType: { kind: string, time_in_force: string } | null,
 *   entry: unknown, stop: unknown, risk: unknown, percent: boolean }} ticket
 */
export function sizingArgs({ symbol, side, orderType, entry, stop, risk, percent }) {
  if (!symbol || !orderType || !positive(entry) || !positive(stop) || !positive(risk)) return null;
  return {
    symbol,
    side,
    kind: orderType.kind,
    timeInForce: orderType.time_in_force,
    entry: plain(entry),
    stop: plain(stop),
    risk: plain(risk),
    percent: Boolean(percent),
  };
}

/**
 * The exits to send with the ticket's order as `place_bracket` takes them,
 * or null for a plain order: a stop-loss price, a take-profit price, or both.
 */
export function exitsOf(stop, takeProfit) {
  const stopPrice = positive(stop) ? plain(stop) : null;
  const target = positive(takeProfit) ? plain(takeProfit) : null;
  if (!stopPrice && !target) return null;
  return { stopPrice, trail: null, trailPercent: false, takeProfit: target };
}

/**
 * How core's answer reads: a message id for the case it is. A cap that
 * leaves nothing says so; it is not the budget falling short.
 */
export function sizingMessage(sizing) {
  const none = Number(sizing.quantity) <= 0;
  if (sizing.capped_by) return none ? "ticket.risk.cappedNone" : "ticket.risk.capped";
  return none ? "ticket.risk.none" : "ticket.risk.sized";
}

/** The kept budget, whatever was stored: `{ risk, percent }`. */
export function keptBudget(stored) {
  const risk = positive(stored?.risk) ? String(stored.risk) : "";
  return { risk, percent: stored?.percent === true };
}
