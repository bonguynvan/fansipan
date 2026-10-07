<script>
  import { Grid, tradecanvasTheme } from "bo-grid";
  import { market } from "./market.svelte.js";
  import { isLinked, selectSymbol, symbolOf } from "./links.svelte.js";
  import { t, tn } from "../i18n/i18n.svelte.js";

  let { params, panel } = $props();

  const selected = $derived(symbolOf(params, market.symbols[0]));

  /** Linked: the whole group follows. Unlinked: only this panel remembers it. */
  function choose(symbol) {
    if (isLinked(params.link)) selectSymbol(params.link, symbol);
    else panel?.updateParameters({ symbol });
  }

  const columns = $derived([
    { type: "text", key: "symbol", header: t("column.symbol"), width: 70 },
    { type: "sparkline", key: "trend", sparkKey: "trend", header: t("column.trend"), width: 96, sortable: false },
    { type: "price", key: "bid", header: t("column.bid"), width: 84, flash: "auto" },
    { type: "price", key: "ask", header: t("column.ask"), width: 84, flash: "auto" },
    { type: "number", key: "spread", header: t("column.spread"), width: 64, decimals: 2 },
  ]);

  const rows = $derived(
    market.symbols.map((symbol) => {
      const quote = market.quotes[symbol];
      const bid = Number(quote.bid);
      const ask = Number(quote.ask);
      return { id: symbol, symbol, bid, ask, spread: ask - bid, trend: market.trends[symbol] ?? [] };
    }),
  );
</script>

<div class="watch">
  <header>
    <span class="dim">{tn("watchlist.count", market.symbols.length)}</span>
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
      onRowClick={(row) => choose(row.symbol)}
      emptyMessage={t("feed.waiting")}
    />
  </div>

  <p class="hint dim">{t("watchlist.hint")}</p>
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


  .grid {
    min-height: 0;
  }

  .hint {
    margin: 0;
    padding: 8px 16px 12px;
    font-size: 11px;
  }
</style>
