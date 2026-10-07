<script>
  import { Grid, tradecanvasTheme } from "bo-grid";
  import { mark, money, price, quantity } from "./format.js";
  import { market } from "./market.svelte.js";
  import { selectSymbol } from "./links.svelte.js";

  let { params } = $props();

  const account = $derived(market.account);
  const quotes = $derived(market.quotes);

  const tone = (value) => (value > 0 ? "up" : value < 0 ? "down" : undefined);

  const columns = [
    { type: "text", key: "symbol", header: "Symbol", width: 66, minWidth: 62 },
    { type: "text", key: "quantity", header: "Qty", width: 48, minWidth: 40, align: "right", format: (v) => quantity(v), cellClass: (_v, row) => (Number(row.quantity) < 0 ? "down" : undefined) },
    { type: "text", key: "avg", header: "Avg", width: 78, minWidth: 72, align: "right", format: (v) => price(v) },
    { type: "price", key: "mark", header: "Mark", width: 72, minWidth: 64, flash: "auto" },
    { type: "number", key: "pnl", header: "P&L", width: 76, minWidth: 64, decimals: 2, cellClass: (v) => tone(v) },
  ];

  const rows = $derived(
    account.positions.map((p) => {
      const last = mark(p, quotes[p.symbol]);
      return {
        id: p.symbol,
        symbol: p.symbol,
        quantity: p.quantity,
        avg: p.avg_price,
        mark: last,
        pnl: (last - Number(p.avg_price)) * Number(p.quantity),
      };
    }),
  );
</script>

<div class="portfolio">
  <header>
    <span class="dim">{account.positions.length} positions</span>
    <span class="num">cash {money(account.cash)}</span>
  </header>

  <div class="grid">
    <Grid
      {rows}
      {columns}
      height="100%"
      theme={tradecanvasTheme}
      fitColumns
      cellSelection={false}
      onRowClick={(row) => selectSymbol(params.link, row.symbol)}
      emptyMessage="No positions yet."
    />
  </div>
</div>

<style>
  .portfolio {
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    grid-template-rows: auto 1fr;
    height: 100%;
    min-height: 0;
  }

  header {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    padding: 10px 16px 8px;
  }


  .grid {
    min-height: 0;
  }

  .portfolio :global(.bo-grid .c.up) {
    color: var(--up);
  }

  .portfolio :global(.bo-grid .c.down) {
    color: var(--down);
  }
</style>
