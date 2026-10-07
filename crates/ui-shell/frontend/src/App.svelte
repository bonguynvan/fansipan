<script>
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import Watchlist from "./lib/Watchlist.svelte";
  import OrderTicket from "./lib/OrderTicket.svelte";
  import Portfolio from "./lib/Portfolio.svelte";
  import Blotter from "./lib/Blotter.svelte";
  import { mark, money } from "./lib/format.js";

  let quotes = $state({});
  let orders = $state({});
  let account = $state({ cash: "0", positions: [] });
  let ready = $state(false);
  let selected = $state(null);

  function applyTick(tick) {
    if (tick.kind === "quote") {
      quotes[tick.data.symbol] = tick.data;
      selected ??= tick.data.symbol;
    } else if (tick.kind === "order") {
      orders[tick.data.id] = tick.data;
    } else if (tick.kind === "account") {
      account = tick.data;
    }
  }

  onMount(() => {
    let unlisten;
    (async () => {
      unlisten = await listen("bus-event", (event) => applyTick(event.payload));
      const snap = await invoke("snapshot");
      for (const q of snap.quotes) quotes[q.symbol] = q;
      for (const o of snap.orders) orders[o.id] = o;
      account = snap.account;
      selected ??= snap.quotes[0]?.symbol ?? null;
      ready = true;
    })();
    return () => unlisten?.();
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
      <span class="mark">◆</span>
      <span>Trading Terminal</span>
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
    <section class="pane watch">
      <Watchlist {quotes} {symbols} bind:selected />
    </section>

    <section class="pane ticket">
      <OrderTicket {selected} quote={selected ? quotes[selected] : null} />
    </section>

    <section class="pane portfolio">
      <Portfolio {account} {quotes} />
    </section>

    <section class="pane blotter">
      <Blotter orders={orderList} />
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

  main {
    display: grid;
    grid-template-columns: minmax(360px, 1.4fr) minmax(300px, 1fr);
    grid-template-rows: minmax(0, 1.3fr) minmax(0, 1fr);
    gap: 1px;
    background: var(--rule);
    min-height: 0;
  }

  .pane {
    background: var(--panel);
    min-height: 0;
    overflow: auto;
  }

  .watch {
    grid-row: 1 / span 2;
  }

  .blotter {
    grid-column: 2;
  }
</style>
