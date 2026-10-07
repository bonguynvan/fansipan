// Prices, quantities, and cash arrive as exact decimal strings from the shell.
// These helpers are display-only: nothing computed here goes back to Rust.

const PRICE = { minimumFractionDigits: 2, maximumFractionDigits: 4 };
const QUANTITY = { maximumFractionDigits: 8 };
const DEFAULT_MONEY_DIGITS = 2;

const fixed = (digits) => ({ minimumFractionDigits: digits, maximumFractionDigits: digits });

function format(value, options) {
  if (value === null || value === undefined || value === "") return "—";
  return Number(value).toLocaleString("en-US", options);
}

/** Minor-unit digits of an ISO 4217 currency: 2 for USD, 0 for VND. */
export function currencyDigits(currency) {
  if (!currency) return DEFAULT_MONEY_DIGITS;
  try {
    return new Intl.NumberFormat("en-US", { style: "currency", currency }).resolvedOptions().maximumFractionDigits;
  } catch {
    return DEFAULT_MONEY_DIGITS;
  }
}

/** With `decimals` (from the instrument's tick), exactly that many places. */
export const price = (value, decimals) => format(value, decimals === undefined ? PRICE : fixed(decimals));
export const money = (value, currency) => format(value, fixed(currencyDigits(currency)));
export const quantity = (value) => format(value, QUANTITY);

/** Liquidation mark: a long sells at the bid, a short buys back at the ask. */
export function mark(position, quote) {
  if (!quote) return Number(position.avg_price);
  return Number(position.quantity) >= 0 ? Number(quote.bid) : Number(quote.ask);
}
