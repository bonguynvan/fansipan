<script>
  import { Grid, tradecanvasTheme } from "bo-grid";
  import { mark, money, price, quantity } from "./format.js";
  import { market } from "./market.svelte.js";
  import { selectSymbol } from "./links.svelte.js";
  import { t, tn } from "../i18n/i18n.svelte.js";

  let { params } = $props();

  const account = $derived(market.account);
  const quotes = $derived(market.quotes);

  const tone = (value) => (value > 0 ? "up" : value < 0 ? "down" : undefined);
  const atTick = (value, row) => price(value, row && market.priceDecimals(row.symbol));
  const inCash = (value) => money(value, market.info?.currency);

  const columns = $derived([
    { type: "text", key: "symbol", header: t("column.symbol"), width: 66, minWidth: 62 },
    { type: "text", key: "quantity", header: t("column.qty"), width: 48, minWidth: 40, align: "right", format: (v) => quantity(v), cellClass: (_v, row) => (Number(row.quantity) < 0 ? "down" : undefined) },
    { type: "text", key: "avg", header: t("column.avg"), width: 78, minWidth: 72, align: "right", format: atTick },
    { type: "price", key: "mark", header: t("column.mark"), width: 72, minWidth: 64, flash: "auto", format: atTick },
    { type: "number", key: "pnl", header: t("column.pnl"), width: 76, minWidth: 64, format: inCash, cellClass: (v) => tone(v) },
  ]);

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
    <span class="dim">{tn("account.positions", account.positions.length)}</span>
    <span class="num">{t("account.cash", { cash: inCash(account.cash) })}</span>
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
      emptyMessage={t("account.empty")}
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
