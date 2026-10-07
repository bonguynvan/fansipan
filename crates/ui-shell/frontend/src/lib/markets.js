// Display conventions per market, keyed by the market id the shell reports
// in its snapshot. Market *rules* (ticks, lots, limits) never live here:
// they come from the adapter as data, and words never live here either:
// they come from the message catalogs, in the reader's language. A preset
// only says how a market's prices are usually read. Markets without one
// get FALLBACK.
// Register your own from screens/index.js; see docs/customizing.md.

const ID = /^[a-z][a-z0-9-]{0,39}$/;

const FALLBACK = Object.freeze({
  id: "",
  /** A board shows prices divided by this (VN boards quote in thousands of VND). */
  priceScale: 1,
  /** Decimal places a board shows prices with; null to follow each instrument's tick. */
  priceDecimals: null,
  /** `bo-grid/trading` session windows for the board's session badge, or null for none. */
  schedule: null,
  /** IANA zone the schedule's times are in. */
  timeZone: null,
  /** Layout a fresh main window opens with; null for the default layout. */
  layout: null,
});

const presets = new Map();

/**
 * @param {object} preset
 * @param {string} preset.id   The market id the shell reports (e.g. "vn").
 * @param {number} [preset.priceScale]
 * @param {number|null} [preset.priceDecimals]
 * @param {Array<{start: string, end: string, state: string}>|null} [preset.schedule]
 * @param {string|null} [preset.timeZone]
 * @param {string|null} [preset.layout]
 */
export function registerMarketPreset(preset) {
  if (typeof preset?.id !== "string" || !ID.test(preset.id)) {
    throw new Error(`market preset id "${preset?.id}" must be 1-40 of a-z, 0-9, '-', starting with a letter`);
  }
  if (presets.has(preset.id)) throw new Error(`market preset "${preset.id}" is already registered`);
  const merged = { ...FALLBACK, ...preset };
  if (!(merged.priceScale > 0)) throw new Error(`market preset "${preset.id}": priceScale must be positive`);
  if (merged.priceDecimals !== null && !(Number.isInteger(merged.priceDecimals) && merged.priceDecimals >= 0)) {
    throw new Error(`market preset "${preset.id}": priceDecimals must be a whole number or null`);
  }
  if (merged.schedule !== null && !Array.isArray(merged.schedule)) {
    throw new Error(`market preset "${preset.id}": schedule must be an array of session windows or null`);
  }
  presets.set(preset.id, Object.freeze(merged));
}

/** The preset for a market id, or the plain fallback. */
export function marketPreset(id) {
  return presets.get(id) ?? FALLBACK;
}
