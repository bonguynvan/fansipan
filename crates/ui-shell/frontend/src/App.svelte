<script>
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { onBatch, startBridge } from "./lib/bridge.js";
  import { createSync } from "./lib/sync.js";
  import Watchlist from "./lib/Watchlist.svelte";
  import Chart from "./lib/Chart.svelte";
  import OrderTicket from "./lib/OrderTicket.svelte";
  import Portfolio from "./lib/Portfolio.svelte";
  import Blotter from "./lib/Blotter.svelte";
  import { mark, money } from "./lib/format.js";
  import { TREND_BARS, toCandle, withBars, withHistory } from "./lib/trends.js";

  // Plain objects replaced on every change: cheap to diff, and what bo-grid
  // expects for rows it flashes.
  let quotes = $state.raw({});
  let orders = $state.raw({});
  let trends = $state.raw({});
  let account = $state.raw({ cash: "0", positions: [] });
  let ready = $state(false);
  let selected = $state(null);

  function applyBatch(batch) {
    if (batch.quotes.length) {
      quotes = { ...quotes, ...Object.fromEntries(batch.quotes.map((q) => [q.symbol, q])) };
      selected ??= batch.quotes[0].symbol;
    }
    if (batch.orders.length) {
      orders = { ...orders, ...Object.fromEntries(batch.orders.map((o) => [o.id, o])) };
    }
    if (batch.account) account = batch.account;
    if (batch.bars.length) trends = withBars(trends, batch.bars);
  }

  async function loadTrends(symbols) {
    const loaded = await Promise.all(
      symbols.map(async (symbol) => [
        symbol,
        (await invoke("bars", { symbol, limit: TREND_BARS })).map(toCandle),
      ]),
    );
    // Batches keep arriving while history loads.
    trends = withHistory(trends, loaded);
  }

  const sync = createSync(applyBatch);

  onMount(() => {
    const stop = onBatch(sync.batch);
    (async () => {
      await startBridge();
      const snap = await invoke("snapshot");
      sync.snapshot(snap);
      await loadTrends(snap.quotes.map((q) => q.symbol));
      ready = true;
    })();
    return stop;
  });

  const symbols = $derived(Object.keys(quotes).sort());
  const orderList = $derived(Object.values(orders).sort((a, b) => b.id - a.id));
  const equity = $derived(
    Number(account.cash) +
      account.positions.reduce(
        (sum, p) => sum + Number(p.quantity) * mark(p, quotes[p.symbol]),
        0,
      ),
  );
</script>

<div class="terminal">
  <header>
    <div class="brand">
      <span class="mark" aria-hidden="true">▲</span>
      <span>Fansipan</span>
      <span class="venue dim">sim · simulated venue</span>
    </div>
    <div class="equity">
      <span class="dim">equity</span>
      <span class="num">{money(equity)}</span>
    </div>
    <div class="status" class:live={ready}>
      <span class="dot"></span>{ready ? "live" : "connecting"}
    </div>
  </header>

  <main>
    <section class="pane watch" aria-label="Watchlist">
      <Watchlist {quotes} {trends} {symbols} bind:selected />
    </section>

    <section class="pane chart" aria-label="Chart">
      <Chart symbol={selected} />
    </section>

    <section class="pane ticket" aria-label="Order ticket">
      <OrderTicket {selected} quote={selected ? quotes[selected] : null} />
    </section>

    <section class="pane blotter" aria-label="Orders">
      <Blotter orders={orderList} />
    </section>

    <section class="pane portfolio" aria-label="Account">
      <Portfolio {account} {quotes} />
    </section>
  </main>
</div>

<style>
  .terminal {
    display: grid;
    grid-template-rows: auto 1fr;
    height: 100vh;
  }

  header {
    display: grid;
    grid-template-columns: 1fr auto 1fr;
    align-items: center;
    padding: 10px 16px;
    background: var(--panel);
    border-bottom: 1px solid var(--rule);
  }

  .brand {
    display: flex;
    gap: 10px;
    align-items: baseline;
    font-weight: 600;
    letter-spacing: 0.02em;
  }

  .mark {
    color: var(--accent);
  }

  .venue {
    font-weight: 400;
    font-size: 11px;
  }

  .equity {
    display: flex;
    gap: 10px;
    align-items: baseline;
    font-size: 18px;
  }

  .equity .dim {
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.12em;
  }

  .status {
    justify-self: end;
    display: flex;
    gap: 8px;
    align-items: center;
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.12em;
    color: var(--text-dim);
  }

  .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--text-dim);
  }

  .status.live {
    color: var(--up);
  }

  .status.live .dot {
    background: var(--up);
    box-shadow: 0 0 0 3px color-mix(in srgb, var(--up) 25%, transparent);
  }

  /* Watchlist, chart, and ticket on top; the account under the watchlist
     and the orders across the rest of the bottom, where every column fits.
     The chart gets the space; the side columns stay at the width their
     tables need. */
  main {
    display: grid;
    grid-template-columns: minmax(380px, 25%) minmax(0, 1fr) minmax(360px, 24%);
    grid-template-rows: minmax(0, 1.75fr) minmax(0, 1fr);
    grid-template-areas:
      "watch chart ticket"
      "portfolio blotter blotter";
    gap: 1px;
    background: var(--rule);
    min-height: 0;
  }

  .pane {
    background: var(--panel);
    min-height: 0;
    min-width: 0;
    overflow: hidden;
  }

  .watch { grid-area: watch; }
  .chart { grid-area: chart; }
  .ticket { grid-area: ticket; overflow: auto; }
  .blotter { grid-area: blotter; }
  .portfolio { grid-area: portfolio; }
</style>
