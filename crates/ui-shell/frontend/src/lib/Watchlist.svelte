<script>
  import { Grid, tradecanvasTheme } from "bo-grid";

  let { quotes, trends, symbols, selected = $bindable(null) } = $props();

  const columns = [
    { type: "text", key: "symbol", header: "Symbol", width: 70 },
    { type: "sparkline", key: "trend", sparkKey: "trend", header: "2 min", width: 96, sortable: false },
    { type: "price", key: "bid", header: "Bid", width: 84, flash: "auto" },
    { type: "price", key: "ask", header: "Ask", width: 84, flash: "auto" },
    { type: "number", key: "spread", header: "Spread", width: 64, decimals: 2 },
  ];

  const rows = $derived(
    symbols.map((symbol) => {
      const quote = quotes[symbol];
      const bid = Number(quote.bid);
      const ask = Number(quote.ask);
      return { id: symbol, symbol, bid, ask, spread: ask - bid, trend: trends[symbol] ?? [] };
    }),
  );
</script>

<div class="watch">
  <header>
    <h2>Watchlist</h2>
    <span class="dim">{symbols.length} instruments</span>
  </header>

  <div class="grid">
    <Grid
      {rows}
      {columns}
      height="100%"
      theme={tradecanvasTheme}
      fitColumns
      cellSelection={false}
      selectedRowId={selected}
      onRowClick={(row) => (selected = row.symbol)}
      emptyMessage="Waiting for the first quotes…"
    />
  </div>

  <p class="hint dim">Click a row to chart it and load the order ticket.</p>
</div>

<style>
  .watch {
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    grid-template-rows: auto 1fr auto;
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

  .hint {
    margin: 0;
    padding: 8px 16px 12px;
    font-size: 11px;
  }
</style>
