// Tick-ladder arithmetic for input helpers (the order ticket's price
// stepper). Display assistance only: the engines check every order against
// the same rules in exact decimals, so nothing here has to be authoritative.

const MAX_SETTLE_PASSES = 5;
/** Below this many tick units, a quotient is float noise from a whole one. */
const UNIT_EPSILON = 1e-6;
const DEFAULT_TICK = 0.01;

/** The instrument's ladder as numbers, ascending: `[{ from, tick }]`. */
export function tickTable(instrument) {
  const steps = (instrument?.ticks ?? []).map((s) => ({ from: Number(s.from), tick: Number(s.tick) }));
  return steps.length ? steps : [{ from: 0, tick: DEFAULT_TICK }];
}

export function tickAt(table, price) {
  let tick = table[0].tick;
  for (const step of table) if (price >= step.from) tick = step.tick;
  return tick;
}

export function finestTick(table) {
  return Math.min(...table.map((s) => s.tick));
}

/**
 * Decimal places that show every tradable price, e.g. 2 for a cent tick and
 * 0 for 10 VND; with prices shown divided by `scale`, the places of the
 * scaled tick (10 VND in thousands: 0.01, so 2).
 */
export function tickDecimals(table, scale = 1) {
  return decimalsOf(finestTick(table) / scale);
}

function decimalsOf(tick) {
  const text = tick.toFixed(10).replace(/0+$/, "");
  const dot = text.indexOf(".");
  return dot === -1 ? 0 : text.length - dot - 1;
}

function onTick(price, tick, mode) {
  const units = price / tick;
  const whole = Math.round(units);
  const snapped = Math.abs(units - whole) < UNIT_EPSILON ? whole : units;
  const rounded = mode === "down" ? Math.floor(snapped) : mode === "up" ? Math.ceil(snapped) : Math.round(snapped);
  return Number((rounded * tick).toFixed(decimalsOf(tick)));
}

/**
 * The nearest tradable price (`mode` "nearest"), or the one at or below
 * ("down") or at or above ("up"). Rounding can move a price into another
 * step of the ladder, so it settles on the tick of where it lands.
 */
export function roundPrice(table, price, mode = "nearest") {
  let tick = tickAt(table, price);
  let candidate = onTick(price, tick, mode);
  for (let pass = 0; pass < MAX_SETTLE_PASSES; pass++) {
    const there = tickAt(table, candidate);
    if (there === tick) break;
    tick = there;
    candidate = onTick(price, tick, mode);
  }
  return candidate;
}

function onLadder(table, price) {
  return roundPrice(table, price) === price;
}

/** The next tradable price above `price`; from an off-tick price, the first one above it. */
export function stepUp(table, price) {
  return onLadder(table, price) ? roundPrice(table, price + finestTick(table), "up") : roundPrice(table, price, "up");
}

/** The next tradable price below `price`, or `price` itself when nothing positive is below it. */
export function stepDown(table, price) {
  const below = onLadder(table, price)
    ? roundPrice(table, price - finestTick(table), "down")
    : roundPrice(table, price, "down");
  return below > 0 ? below : price;
}

/** Whole lots only, at least one. */
export function roundLots(quantity, lot) {
  if (!(lot > 0)) return quantity;
  return Math.max(lot, Math.round(quantity / lot) * lot);
}
