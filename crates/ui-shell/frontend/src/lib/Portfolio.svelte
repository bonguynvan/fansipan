<script>
  import { Grid } from "bo-grid";
  import { ROW_HEIGHT_PX, gridTheme } from "./gridTheme.js";
  import { px } from "./ui.svelte.js";
  import { mark, money, price, quantity } from "./format.js";
  import { market } from "./market.svelte.js";
  import { chooseSymbol } from "./links.svelte.js";
  import { t, tn } from "../i18n/i18n.svelte.js";

  let { params, panel } = $props();

  const account = $derived(market.account);
  const quotes = $derived(market.quotes);

  const tone = (value) => (value > 0 ? "up" : value < 0 ? "down" : undefined);
  const atTick = (value, row) => price(value, row && market.priceDecimals(row.symbol));
  const inCash = (value) => money(value, market.info?.currency);

  const columns = $derived([
    { type: "text", key: "symbol", header: t("column.symbol"), width: px(66), minWidth: px(62) },
    { type: "text", key: "quantity", header: t("column.qty"), width: px(48), minWidth: px(40), align: "right", format: (v) => quantity(v), cellClass: (_v, row) => (Number(row.quantity) < 0 ? "down" : undefined) },
    { type: "text", key: "avg", header: t("column.avg"), width: px(78), minWidth: px(72), align: "right", format: atTick },
    { type: "price", key: "mark", header: t("column.mark"), width: px(72), minWidth: px(64), flash: "auto", format: atTick },
    { type: "number", key: "pnl", header: t("column.pnl"), width: px(76), minWidth: px(64), format: inCash, cellClass: (v) => tone(v) },
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
      theme={gridTheme}
      rowHeight={px(ROW_HEIGHT_PX)}
      fitColumns
      cellSelection={false}
      onRowClick={(row) => chooseSymbol(params, panel, row.symbol)}
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
    padding: 0.625rem 1rem 0.5rem;
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
