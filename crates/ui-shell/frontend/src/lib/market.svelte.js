// Market and account state for one window, fed by the shell: the startup
// snapshot, then the numbered batches. Every screen in the window reads
// from here; none of them talks to the shell for data on its own.

import { invoke } from "@tauri-apps/api/core";
import { onBatch, startBridge } from "./bridge.js";
import { createSync } from "./sync.js";
import { TREND_BARS, toCandle, withBars, withHistory } from "./trends.js";

class Market {
  // Plain objects replaced on every change: cheap to diff, and what
  // bo-grid expects for rows it flashes.
  quotes = $state.raw({});
  orders = $state.raw({});
  trends = $state.raw({});
  account = $state.raw({ cash: "0", positions: [] });
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

/** Starts this window's feed once; later calls return the same promise. */
export function startMarket() {
  started ??= (async () => {
    const sync = createSync((batch) => market.apply(batch));
    onBatch(sync.batch);
    await startBridge();
    const snap = await invoke("snapshot");
    sync.snapshot(snap);
    await loadTrends(snap.quotes.map((q) => q.symbol));
    market.ready = true;
  })();
  return started;
}
