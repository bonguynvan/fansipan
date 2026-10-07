// Time and sales: each symbol's recent prints, numbered by the engine in
// the order its tape took them. A number is never reused, so it identifies
// a print. Prints arrive two ways: the `prints` read when a screen first
// shows a symbol, and every batch. The two overlap, so they are merged by
// number, and the same print never shows twice.

import { bandsOf, toneOf } from "./board.js";

/** Prints kept per symbol: how far back a time-and-sales screen scrolls. */
export const TAPE_PRINTS = 200;

/** `tape` with `prints` merged in by number, oldest first, newest TAPE_PRINTS kept. */
function merged(tape, prints) {
  const bySeq = new Map(tape.map((print) => [print.seq, print]));
  for (const print of prints) bySeq.set(print.seq, print);
  return [...bySeq.values()].sort((a, b) => a.seq - b.seq).slice(-TAPE_PRINTS);
}

function groupBySymbol(prints) {
  const groups = new Map();
  for (const print of prints) {
    if (!groups.has(print.symbol)) groups.set(print.symbol, []);
    groups.get(print.symbol).push(print);
  }
  return groups;
}

/** Every tape with this batch's prints folded in. Never mutates `tapes`. */
export function withPrints(tapes, prints) {
  const updates = [...groupBySymbol(prints)].map(([symbol, incoming]) => [
    symbol,
    merged(tapes[symbol] ?? [], incoming),
  ]);
  return updates.length ? { ...tapes, ...Object.fromEntries(updates) } : tapes;
}

/** The tapes with one symbol's read history merged under what the batches already brought. */
export function withTapeHistory(tapes, symbol, history) {
  return { ...tapes, [symbol]: merged(history, tapes[symbol] ?? []) };
}

/**
 * Grid rows for a tape, newest first. Prices are divided by `scale` for
 * display; each row's tone is resolved on the exact price against the
 * board's reference, ceiling and floor, as the price board does.
 */
export function printRows(tape, board, scale = 1) {
  const bands = board ? bandsOf(board) : null;
  return tape
    .map((print) => ({
      id: print.seq,
      time: print.time_ms,
      price: Number(print.price) / scale,
      quantity: Number(print.quantity),
      side: print.side,
      tone: toneOf(Number(print.price), bands),
    }))
    .reverse();
}
