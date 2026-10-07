<script>
  import { Grid, tradecanvasTheme } from "bo-grid";
  import CancelCell from "./CancelCell.svelte";
  import { blotterError } from "./orders.svelte.js";
  import { price, quantity } from "./format.js";
  import { market } from "./market.svelte.js";
  import { venueNameOf } from "./sessions.js";
  import { orderTypeName } from "./orderTypes.js";
  import { t } from "../i18n/i18n.svelte.js";

  const orders = $derived(market.orderList);

  const STATUS_TONES = {
    filled: "up",
    partial: "up",
    accepted: "amber",
    submitted: "amber",
    cancelling: "neutral",
    amending: "amber",
    cancelled: "neutral",
    rejected: "down",
    denied: "down",
  };

  // Badges colour by the text they show, so the tone maps are keyed by the
  // translated words.
  const byText = (prefix, tones) =>
    Object.fromEntries(Object.entries(tones).map(([key, tone]) => [t(`${prefix}.${key}`), tone]));

  const columns = $derived([
    { type: "number", key: "id", header: t("column.id"), width: 44, decimals: 0 },
    { type: "text", key: "symbol", header: t("column.symbol"), width: 64 },
    { type: "badge", key: "sideText", header: t("column.side"), width: 64, tones: byText("side", { buy: "up", sell: "down" }) },
    { type: "text", key: "kind", header: t("column.type"), width: 150, tooltip: true },
    { type: "text", key: "filled", header: t("column.filled"), width: 124, align: "right", flash: "change" },
    { type: "text", key: "avg", header: t("column.avg"), width: 92, align: "right", format: (v, row) => price(v, row && market.priceDecimals(row.symbol)) },
    { type: "badge", key: "statusText", header: t("column.status"), width: 116, tones: byText("status", STATUS_TONES), flash: "change" },
    { type: "text", key: "reason", header: t("column.reason"), width: 220, tooltip: true },
    { type: "custom", key: "action", header: "", width: 84, sortable: false, component: CancelCell },
  ]);

  const rows = $derived(
    orders.map((o) => ({
      id: o.id,
      symbol: o.symbol,
      sideText: t(`side.${o.side}`),
      kind: typeText(o),
      filled: `${quantity(o.filled_quantity)}/${quantity(o.quantity)}`,
      avg: o.avg_fill_price,
      status: o.status,
      statusText: t(`status.${o.status}`),
      reason: o.reason ?? "",
      open: o.open,
    })),
  );

  /** The order's type as its market names it, with the limit price if it has one. */
  function typeText(order) {
    const name = venueNameOf(order, market.instruments[order.symbol]?.exchange, market.typeNames);
    const type = orderTypeName({ ...order, name }, t);
    return order.limit_price
      ? t("order.typeAndPrice", { type, price: price(order.limit_price, market.priceDecimals(order.symbol)) })
      : type;
  }

  const working = $derived(orders.filter((o) => o.open).length);
</script>

<div class="blotter">
  <header>
    <span class="dim">{t("orders.summary", { working, total: orders.length })}</span>
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
      emptyMessage={t("orders.empty")}
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
