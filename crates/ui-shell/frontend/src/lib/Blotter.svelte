<script>
  import { tick } from "svelte";
  import { Grid } from "bo-grid";
  import { keyboardRow } from "./keyboardRow.js";
  import { ROW_HEIGHT_PX, gridTheme } from "./gridTheme.js";
  import { px, ui } from "./ui.svelte.js";
  import { sizeMarks } from "./panelSize.svelte.js";
  import OrderActions from "./OrderActions.svelte";
  import AmendForm from "./AmendForm.svelte";
  import { canAmendOrder } from "./amend.js";
  import { blotterError } from "./orders.svelte.js";
  import { VIEWS, ordersIn, viewCounts, viewOf } from "./blotter.js";
  import { CONFIRM_MS, cancelAllOrders } from "./tradingActions.js";
  import Chips from "./Chips.svelte";
  import { price, quantity } from "./format.js";
  import { market } from "./market.svelte.js";
  import { venueNameOf } from "./sessions.js";
  import { orderTypeName } from "./orderTypes.js";
  import { t, tn } from "../i18n/i18n.svelte.js";

  let { params, panel } = $props();

  /** How long the result of cancelling every working order stays shown. */
  const RESULT_MS = 6000;
  const EMPTY = { working: "orders.noneWorking", finished: "orders.noneFinished", all: "orders.empty" };
  /** The cancel-all results that left orders working. */
  const FAILED = new Set(["hotkey.cancelFailed", "orders.cancelAllFailed"]);

  // Working orders to act on, finished ones to look back on, or all; each
  // blotter remembers its own view.
  const view = $derived(viewOf(params));
  const counts = $derived(viewCounts(market.orderList));
  const viewOptions = $derived(VIEWS.map((name) => ({ value: name, text: t(`orders.view.${name}`, { count: counts[name] }) })));
  const orders = $derived(ordersIn(view, market.orderList));

  // Cancelling every working order takes a second click within the same
  // two seconds a second key press gets: a stray click would cost every
  // order its place in the queue.
  let armed = $state(false);
  let result = $state(null);
  let armTimer = null;
  let resultTimer = null;

  let destroyed = false;
  let header;

  async function cancelAll() {
    clearTimeout(armTimer);
    if (!armed) {
      armed = true;
      result = null;
      armTimer = setTimeout(() => (armed = false), CONFIRM_MS);
      return;
    }
    armed = false;
    let notice;
    try {
      notice = await cancelAllOrders(null);
    } catch (err) {
      notice = { id: "orders.cancelAllFailed", vars: { error: String(err) } };
    }
    if (destroyed) return;
    result = { ...notice, failed: FAILED.has(notice.id) };
    clearTimeout(resultTimer);
    resultTimer = setTimeout(() => (result = null), RESULT_MS);
    // With nothing left working the button goes; the keyboard stays in the
    // blotter, on the view it shows.
    await tick();
    if (header && !header.querySelector(".cancel-all")) header.querySelector('[aria-pressed="true"]')?.focus();
  }

  $effect(() => () => {
    destroyed = true;
    clearTimeout(armTimer);
    clearTimeout(resultTimer);
  });

  /** The order this blotter's amend form is open for, by id. */
  let amending = $state(null);
  const amendingOrder = $derived(amending === null ? null : market.orders[amending]);
  let blotter;

  // The form closes once its order is no longer shown here: it finished,
  // or the trader switched to a view without it.
  $effect(() => {
    if (amending !== null && !orders.some((order) => order.id === amending)) closeAmend();
  });

  /** Back to the Amend button that opened the form, or the grid once the order has none. */
  async function closeAmend() {
    const id = amending;
    amending = null;
    await tick();
    const target = blotter?.querySelector(`[data-amend="${id}"]`) ?? blotter?.querySelector('[role="grid"]');
    target?.focus();
  }

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

  // Which columns fit: the reason goes first, then the id, type, and average.
  let width = $state(0);
  const marks = $derived(sizeMarks(width, ui.rootPx));
  const fits = (column) => !(column.leavesWhen && marks[column.leavesWhen]);

  const allColumns = $derived([
    { type: "number", key: "id", header: t("column.id"), width: px(44), decimals: 0, leavesWhen: "narrow" },
    { type: "text", key: "symbol", header: t("column.symbol"), width: px(64) },
    { type: "badge", key: "sideText", header: t("column.side"), width: px(64), tones: byText("side", { buy: "up", sell: "down" }) },
    { type: "text", key: "kind", header: t("column.type"), width: px(150), tooltip: true, leavesWhen: "narrow" },
    { type: "text", key: "filled", header: t("column.filled"), width: px(124), align: "right", flash: "change" },
    { type: "text", key: "avg", header: t("column.avg"), width: px(92), align: "right", format: (v, row) => price(v, row && market.priceDecimals(row.symbol)), leavesWhen: "narrow" },
    { type: "badge", key: "statusText", header: t("column.status"), width: px(116), tones: byText("status", STATUS_TONES), flash: "change" },
    { type: "text", key: "reason", header: t("column.reason"), width: px(220), tooltip: true, leavesWhen: "compact" },
    {
      type: "custom",
      key: "action",
      header: "",
      width: px(164),
      sortable: false,
      component: OrderActions,
      onAmend: (id) => (amending = id),
    },
  ]);
  const columns = $derived(allColumns.filter(fits));

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
      amendable: canAmendOrder(o, market.canAmend),
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

</script>

<div class="blotter" bind:this={blotter} bind:clientWidth={width}>
  <header bind:this={header}>
    <Chips label={t("orders.view")} options={viewOptions} value={view} onchange={(next) => panel?.updateParameters({ view: next })} />
    {#if counts.working > 0 || armed}
      <button type="button" class="cancel-all" class:armed onclick={cancelAll}>
        {armed ? t("orders.confirmCancelAll", { count: counts.working }) : t("orders.cancelAll")}
      </button>
    {/if}
  </header>

  <!-- One status line: what the next click will do while armed, then what was done. -->
  {#if armed}
    <p class="result" role="status">{t("hotkey.confirmCancelAll")}</p>
  {:else if result}
    <p class="result" class:failed={result.failed} role="status">
      {result.count === undefined ? t(result.id, result.vars) : tn(result.id, result.count, result.vars)}
    </p>
  {/if}

  {#if blotterError.text}
    <p class="error" role="alert">{blotterError.text}</p>
  {/if}

  {#if amendingOrder}
    {#key amending}
      <AmendForm order={amendingOrder} onclose={closeAmend} />
    {/key}
  {/if}

  <div class="grid" use:keyboardRow>
    <Grid
      {rows}
      {columns}
      height="100%"
      theme={gridTheme}
      rowHeight={px(ROW_HEIGHT_PX)}
      fitColumns
      cellSelection={false}
      onRowClick={(row, event) => {
        // Enter on the row the keyboard is on opens its amend form; a click
        // on a row does nothing, so the mouse never opens one by surprise.
        if (event instanceof KeyboardEvent && row.amendable) amending = row.id;
      }}
      emptyMessage={t(counts.all ? EMPTY[view] : "orders.empty")}
    />
  </div>
</div>

<style>
  .blotter {
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    grid-template-rows: auto auto auto auto minmax(0, 1fr);
    height: 100%;
    min-height: 0;
  }

  header {
    display: flex;
    flex-wrap: wrap;
    justify-content: space-between;
    align-items: center;
    gap: 0.375rem 0.75rem;
    padding: 0.625rem 1rem 0.5rem;
  }

  .cancel-all {
    padding: 0.1875rem 0.625rem;
    font: 600 var(--text-xs)/1.4 var(--mono);
    letter-spacing: 0.04em;
    background: transparent;
    border: 1px solid var(--rule);
    color: var(--text-dim);
    cursor: pointer;
  }

  .cancel-all:hover,
  .cancel-all:focus-visible {
    color: var(--down);
    border-color: var(--down);
    outline: none;
  }

  /* Armed: the next click cancels every working order. */
  .cancel-all.armed {
    color: var(--ink);
    background: var(--down);
    border-color: var(--down);
  }

  .result {
    margin: 0 1rem 0.5rem;
    padding: 0.375rem 0.625rem;
    font-size: var(--text-sm);
    color: var(--text-dim);
    border-left: 3px solid var(--rule);
  }

  .result.failed {
    color: var(--down);
    border-left-color: var(--down);
  }


  /* A row answers Enter, not a click: no pointer to say otherwise. */
  .grid :global(.bo-grid .row.clickable) {
    cursor: default;
  }

  /* Last row whichever of the result, the error, and the amend form are showing. */
  .grid {
    grid-row: 5;
    min-height: 0;
  }

  .error {
    margin: 0 1rem 0.5rem;
    padding: 0.375rem 0.625rem;
    font-size: var(--text-sm);
    color: var(--down);
    border-left: 3px solid var(--down);
  }
</style>
