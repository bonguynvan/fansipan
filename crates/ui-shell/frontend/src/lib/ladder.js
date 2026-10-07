// A depth ladder: every tradable price of one instrument, highest first,
// with the book's size at each price that has one. With a daily band the
// ladder runs floor to ceiling, which is every price an order can name;
// without one it runs a fixed number of ticks either side of the market.
// Display only: the prices come from the instrument's tick table, the
// sizes from the depth the engines publish.

import { bandsOf, toneOf } from "./board.js";
import { roundPrice, stepDown, stepUp, tickDecimals } from "./ticks.js";

/** Ticks either side of the market when no band bounds the ladder. */
export const LADDER_SPAN = 40;
/** A bound on rows, whatever the band and tick. */
export const MAX_LADDER_ROWS = 2000;

const num = (value) => (value === null || value === undefined || value === "" ? null : Number(value));

/** The market's middle as a number: the touch, else the last match, else the reference. */
function marketPrice(board) {
  const bid = num(board?.bids?.[0]?.price);
  const ask = num(board?.asks?.[0]?.price);
  if (bid !== null && ask !== null) return (bid + ask) / 2;
  return bid ?? ask ?? num(board?.last) ?? num(board?.reference);
}

/** Prices from `low` to `high` on the tick table, highest first; at most `cap` of the lowest. */
function walk(table, low, high, cap = MAX_LADDER_ROWS) {
  const prices = [];
  let price = roundPrice(table, low, "up");
  while (price <= high && prices.length < cap) {
    prices.push(price);
    const next = stepUp(table, price);
    if (next <= price) break;
    price = next;
  }
  return prices.reverse();
}

/** Prices `span` ticks either side of `middle`, highest first, kept inside `floor` and `ceiling`. */
function around(table, middle, span, floor = -Infinity, ceiling = Infinity) {
  let low = roundPrice(table, middle, "down");
  let high = roundPrice(table, middle, "up");
  for (let i = 0; i < span; i++) {
    const down = stepDown(table, low);
    if (down >= floor) low = down;
    const up = stepUp(table, high);
    if (up <= ceiling) high = up;
  }
  return walk(table, low, high);
}

/**
 * Every price the ladder shows, highest first; empty when nothing places
 * it. A band with more prices than MAX_LADDER_ROWS shows those around the
 * market, so the cap never cuts away the prices that are trading.
 */
export function ladderPrices(table, board) {
  const floor = num(board?.floor);
  const ceiling = num(board?.ceiling);
  const middle = marketPrice(board);
  if (floor !== null && ceiling !== null) {
    const all = walk(table, floor, ceiling, MAX_LADDER_ROWS + 1);
    if (all.length <= MAX_LADDER_ROWS || middle === null) return all.slice(0, MAX_LADDER_ROWS);
    return around(table, middle, MAX_LADDER_ROWS / 2 - 1, floor, ceiling);
  }
  return middle === null ? [] : around(table, middle, LADDER_SPAN);
}

/** A price as the key the ladder matches book levels on: fixed to the finest tick's places. */
export const priceKey = (price, places) => Number(price).toFixed(places);

const sizesByPrice = (levels, places) =>
  new Map((levels ?? []).map((level) => [priceKey(level.price, places), num(level.size)]));

/**
 * One row per price, highest first. `price` is divided by `scale` for
 * display; `key` is the exact price to send. Tones are resolved on the
 * unscaled price, as the price board does.
 */
export function ladderRows(table, board, scale = 1) {
  const places = tickDecimals(table);
  const bands = board ? bandsOf(board) : null;
  const bids = sizesByPrice(board?.bids, places);
  const asks = sizesByPrice(board?.asks, places);
  const best = (levels) => (levels?.length ? priceKey(levels[0].price, places) : null);
  const bestBid = best(board?.bids);
  const bestAsk = best(board?.asks);
  const last = num(board?.last) === null ? null : priceKey(board.last, places);
  return ladderPrices(table, board).map((price) => {
    const key = priceKey(price, places);
    return {
      id: key,
      key,
      price: price / scale,
      bid: bids.get(key) ?? null,
      ask: asks.get(key) ?? null,
      tone: toneOf(price, bands),
      last: key === last,
      touch: key === bestBid || key === bestAsk,
    };
  });
}

/**
 * The row to keep the ladder centred on: the best bid, so the spread sits
 * mid-screen; else the best ask, the last match, or the middle row.
 */
export function centreIndex(rows) {
  const candidates = [
    rows.findIndex((row) => row.touch && row.bid !== null),
    rows.findIndex((row) => row.touch),
    rows.findIndex((row) => row.last),
  ];
  return candidates.find((index) => index !== -1) ?? Math.floor(rows.length / 2);
}
