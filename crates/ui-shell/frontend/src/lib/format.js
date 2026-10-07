// Prices, quantities, and cash arrive as exact decimal strings from the shell.
// These helpers are display-only: nothing computed here goes back to Rust.

const PRICE = { minimumFractionDigits: 2, maximumFractionDigits: 4 };
const MONEY = { minimumFractionDigits: 2, maximumFractionDigits: 2 };
const QUANTITY = { maximumFractionDigits: 8 };

function format(value, options) {
  if (value === null || value === undefined || value === "") return "—";
  return Number(value).toLocaleString("en-US", options);
}

export const price = (value) => format(value, PRICE);
export const money = (value) => format(value, MONEY);
export const quantity = (value) => format(value, QUANTITY);

/** Liquidation mark: a long sells at the bid, a short buys back at the ask. */
export function mark(position, quote) {
  if (!quote) return Number(position.avg_price);
  return Number(position.quantity) >= 0 ? Number(quote.bid) : Number(quote.ask);
}
