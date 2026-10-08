// The ticket's question to core about the largest order (ADR 0016). Core
// computes the answer in exact decimals by risk's own rules; this only
// decides what to ask and which price to ask it at.

/**
 * The arguments for the shell's `order_capacity`, or null with nothing to
 * ask about. The order is valued at its typed limit; with none (or a type
 * without a price), core values a buy at the day's ceiling. Not at the
 * touch: that moves with every quote and would ask again on every tick.
 * @param {{ symbol: string | null, side: string, orderType: { kind: string, time_in_force: string } | null,
 *   priced: boolean, limitPrice: unknown }} ticket
 */
export function capacityArgs({ symbol, side, orderType, priced, limitPrice }) {
  if (!symbol || !orderType) return null;
  return {
    symbol,
    side,
    limitPrice: priced && Number(limitPrice) > 0 ? String(limitPrice) : null,
    kind: orderType.kind,
    timeInForce: orderType.time_in_force,
  };
}
