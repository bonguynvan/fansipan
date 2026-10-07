// Rows for a price board, built from the shell's board rows. Tones are
// resolved on the exact prices the shell sent, before any scaling for
// display, so a price that equals the ceiling is always "ceiling".

import { resolveTone } from "bo-grid/trading";

/** Book levels a board shows on each side. */
export const BOARD_DEPTH = 3;

const num = (value) => (value === null || value === undefined || value === "" ? null : Number(value));

/** Reference, ceiling and floor as `resolveTone` wants them; null without a reference. */
export function bandsOf(board) {
  const ref = num(board.reference);
  if (ref === null) return null;
  return { ref, ceiling: num(board.ceiling) ?? undefined, floor: num(board.floor) ?? undefined };
}

/** "ceiling" | "floor" | "ref" | "up" | "down", or undefined when there is nothing to compare. */
export function toneOf(price, bands) {
  return price === null || !bands ? undefined : resolveTone(price, bands);
}

function side(levels, prefix, bands, scale) {
  const cells = Array.from({ length: BOARD_DEPTH }, (_, i) => {
    const price = num(levels[i]?.price);
    const key = `${prefix}${i + 1}`;
    return {
      values: { [key]: price === null ? null : price / scale, [`${key}v`]: num(levels[i]?.size) },
      tones: { [key]: toneOf(price, bands) },
    };
  });
  return {
    values: Object.assign({}, ...cells.map((c) => c.values)),
    tones: Object.assign({}, ...cells.map((c) => c.tones)),
  };
}

/**
 * One grid row. Prices are divided by `scale`; `tones` holds the tone of
 * each price column by key (`b1`..`b3`, `a1`..`a3`, `last`, `high`, `low`).
 */
export function boardRow(board, scale = 1) {
  const bands = bandsOf(board);
  const scaled = (value) => (num(value) === null ? null : num(value) / scale);
  const bids = side(board.bids ?? [], "b", bands, scale);
  const asks = side(board.asks ?? [], "a", bands, scale);
  const last = num(board.last);
  return {
    id: board.symbol,
    symbol: board.symbol,
    exchange: board.exchange,
    ref: scaled(board.reference),
    ceiling: scaled(board.ceiling),
    floor: scaled(board.floor),
    ...bids.values,
    last: scaled(board.last),
    lastv: num(board.last_quantity),
    change: scaled(board.change),
    pct: num(board.change_pct),
    ...asks.values,
    volume: num(board.volume),
    high: scaled(board.high),
    low: scaled(board.low),
    tones: {
      ...bids.tones,
      ...asks.tones,
      last: toneOf(last, bands),
      high: toneOf(num(board.high), bands),
      low: toneOf(num(board.low), bands),
    },
  };
}

/** Rows for every board on `exchange` ("" for all), by symbol. */
export function boardRows(boards, { exchange = "", scale = 1 } = {}) {
  return Object.values(boards)
    .filter((board) => !exchange || board.exchange === exchange)
    .sort((a, b) => a.symbol.localeCompare(b.symbol))
    .map((board) => boardRow(board, scale));
}

/** The boards instruments are listed on, alphabetically. */
export function exchangesOf(instruments) {
  return [...new Set(Object.values(instruments).map((i) => i.exchange).filter(Boolean))].sort();
}
