<script>
  import { Grid, tradecanvasTheme } from "bo-grid";
  import CancelCell from "./CancelCell.svelte";
  import { blotterError } from "./orders.svelte.js";
  import { price, quantity } from "./format.js";

  let { orders } = $props();

  const STATUS_TONES = {
    filled: "up",
    partial: "up",
    accepted: "amber",
    submitted: "amber",
    cancelling: "neutral",
    cancelled: "neutral",
    rejected: "down",
    denied: "down",
  };

  const columns = [
    { type: "number", key: "id", header: "#", width: 44, decimals: 0 },
    { type: "text", key: "symbol", header: "Symbol", width: 64 },
    { type: "badge", key: "side", header: "Side", width: 64, tones: { BUY: "up", SELL: "down" } },
    { type: "text", key: "kind", header: "Type", width: 112 },
    { type: "text", key: "filled", header: "Filled", width: 90, align: "right", flash: "change" },
    { type: "text", key: "avg", header: "Avg", width: 92, align: "right", format: (v) => price(v) },
    { type: "badge", key: "status", header: "Status", width: 100, tones: STATUS_TONES, flash: "change" },
    { type: "text", key: "reason", header: "Reason", width: 300, tooltip: true },
    { type: "custom", key: "action", header: "", width: 84, sortable: false, component: CancelCell },
  ];

  const rows = $derived(
    orders.map((o) => ({
      id: o.id,
      symbol: o.symbol,
      side: o.side.toUpperCase(),
      kind: o.limit_price ? `LMT ${price(o.limit_price)}` : "MKT",
      filled: `${quantity(o.filled_quantity)}/${quantity(o.quantity)}`,
      avg: o.avg_fill_price,
      status: o.status,
      reason: o.reason ?? "",
      open: o.open,
    })),
  );

  const working = $derived(orders.filter((o) => o.open).length);
</script>

<div class="blotter">
  <header>
    <h2>Orders</h2>
    <span class="dim">{working} working · {orders.length} this session</span>
  </header>

  {#if blotterError.text}
    <p class="error" role="alert">{blotterError.text}</p>
  {/if}

  <div class="grid">
    <Grid
      {rows}
      {columns}
      height="100%"
      theme={tradecanvasTheme}
      fitColumns
      cellSelection={false}
      emptyMessage="No orders yet. Use the ticket on the right."
    />
  </div>
</div>

<style>
  .blotter {
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    grid-template-rows: auto auto 1fr;
    height: 100%;
    min-height: 0;
  }

  header {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    padding: 10px 16px 8px;
  }

  h2 {
    margin: 0;
    font-size: 12px;
    text-transform: uppercase;
    letter-spacing: 0.14em;
    color: var(--accent);
  }

  .grid {
    min-height: 0;
  }

  .error {
    margin: 0 16px 8px;
    padding: 6px 10px;
    font-size: 12px;
    color: var(--down);
    border-left: 3px solid var(--down);
  }
</style>
