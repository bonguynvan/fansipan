// Market and account state for one window, fed by the shell: the startup
// snapshot, then the numbered batches. Every screen in the window reads
// from here; none of them talks to the shell for data on its own.

import { invoke } from "@tauri-apps/api/core";
import { onBatch, startBridge } from "./bridge.js";
import { createSync } from "./sync.js";
import { tickDecimals, tickTable } from "./ticks.js";
import { TREND_BARS, toCandle, withBars, withHistory } from "./trends.js";

const bySymbol = (items) => Object.fromEntries(items.map((item) => [item.symbol, item]));

class Market {
  // Plain objects replaced on every change: cheap to diff, and what
  // bo-grid expects for rows it flashes.
  quotes = $state.raw({});
  orders = $state.raw({});
  trends = $state.raw({});
  account = $state.raw({ cash: "0", positions: [] });
  /** How each instrument trades (lot, ticks, board), keyed by symbol. */
  instruments = $state.raw({});
  /** One price-board row per symbol, as the shell built it. */
  boards = $state.raw({});
  /** `{ id, title, currency }` of the market the shell is wired to; null until the snapshot is in. */
  info = $state.raw(null);
  ready = $state(false);

  symbols = $derived(Object.keys(this.quotes).sort());
  orderList = $derived(Object.values(this.orders).sort((a, b) => b.id - a.id));

  apply(batch) {
    if (batch.quotes.length) {
      this.quotes = { ...this.quotes, ...Object.fromEntries(batch.quotes.map((q) => [q.symbol, q])) };
    }
    if (batch.orders.length) {
      this.orders = { ...this.orders, ...Object.fromEntries(batch.orders.map((o) => [o.id, o])) };
    }
    if (batch.account) this.account = batch.account;
    if (batch.bars.length) this.trends = withBars(this.trends, batch.bars);
    if (batch.instruments.length) this.instruments = { ...this.instruments, ...bySymbol(batch.instruments) };
    if (batch.boards.length) this.boards = { ...this.boards, ...bySymbol(batch.boards) };
  }

  /** Places that show every tradable price of `symbol`: 0 for VND shares, 2 for cents. */
  priceDecimals(symbol) {
    return tickDecimals(tickTable(this.instruments[symbol]));
  }
}

export const market = new Market();

async function loadTrends(symbols) {
  const loaded = await Promise.all(
    symbols.map(async (symbol) => [
      symbol,
      (await invoke("bars", { symbol, limit: TREND_BARS })).map(toCandle),
    ]),
  );
  // Batches keep arriving while history loads.
  market.trends = withHistory(market.trends, loaded);
}

let started = null;
let infoIn;
const infoReady = new Promise((resolve) => (infoIn = resolve));

/** The market the shell is wired to, once known; null if the snapshot could not be read. */
export function marketInfo() {
  return infoReady;
}

/** Starts this window's feed once; later calls return the same promise. */
export function startMarket() {
  started ??= (async () => {
    try {
      const sync = createSync((batch) => market.apply(batch));
      onBatch(sync.batch);
      await startBridge();
      const snap = await invoke("snapshot");
      sync.snapshot(snap);
      market.info = snap.market ?? null;
      infoIn(market.info);
      await loadTrends(snap.quotes.map((q) => q.symbol));
      market.ready = true;
    } finally {
      // Whoever waits on the market must not wait forever on a failed start.
      infoIn(market.info);
    }
  })();
  return started;
}
