<script>
  // Time and sales for the linked symbol: every print, newest first, its
  // price coloured against the day's reference, ceiling and floor as on
  // the price board. Prints come from the engines' tape: the screen reads
  // a symbol's history once, and the batches bring the rest.
  import { Grid } from "bo-grid";
  import { keyboardRow } from "./keyboardRow.js";
  import { ROW_HEIGHT_PX, gridTheme } from "./gridTheme.js";
  import { px } from "./ui.svelte.js";
  import { loadTape, market } from "./market.svelte.js";
  import { symbolOf } from "./links.svelte.js";
  import { marketPreset } from "./markets.js";
  import { printRows } from "./tape.js";
  import { panelSize } from "./panelSize.svelte.js";
  import { tickDecimals, tickTable } from "./ticks.js";
  import { timeOfDay } from "./format.js";
  import { t } from "../i18n/i18n.svelte.js";

  let { params } = $props();

  const TONES = ["ceiling", "floor", "ref", "up", "down"];
  const SIDE_TONES = { buy: "ts-up", sell: "ts-down" };

  const selected = $derived(symbolOf(params, market.symbols[0]));
  const preset = $derived(marketPreset(market.info?.id));
  const scale = $derived(preset.priceScale);
  const decimals = $derived(preset.priceDecimals ?? tickDecimals(tickTable(market.instruments[selected]), scale));
  const tape = $derived(selected ? (market.tapes[selected] ?? []) : []);
  const rows = $derived(
    printRows(tape, market.boards[selected], scale).map((row) => ({
      ...row,
      sideText: row.side ? t(`side.${row.side}`) : null,
    })),
  );

  let error = $state(null);

  $effect(() => {
    const symbol = selected;
    if (!symbol) return;
    error = null;
    // A failure that lands after the symbol changed is not this one's.
    loadTape(symbol).catch((err) => {
      if (symbol === selected) error = String(err);
    });
  });

  const fixed = (value, places) =>
    value.toLocaleString("en-US", { minimumFractionDigits: places, maximumFractionDigits: places });

  const columns = $derived([
    { type: "text", key: "time", header: t("column.time"), width: px(84), sortable: false, format: (value) => timeOfDay(value) },
    {
      type: "price",
      key: "price",
      header: t("board.price"),
      width: px(72),
      sortable: false,
      autoWidth: true,
      format: (value) => fixed(value, decimals),
      cellClass: (_value, row) => (row.tone ? `ts-${row.tone}` : undefined),
    },
    {
      type: "volume",
      key: "quantity",
      header: t("board.volume"),
      width: px(76),
      sortable: false,
      format: (value) => value.toLocaleString("en-US"),
    },
    // Plain text, not a badge: a print the venue gave no side leaves the
    // cell blank instead of an empty pill.
    {
      type: "text",
      key: "sideText",
      header: t("column.side"),
      width: px(56),
      sortable: false,
      cellClass: (_value, row) => SIDE_TONES[row.side],
    },
  ]);
</script>

<div class="tape" use:panelSize>
  <header>
    <span class="sym">{selected ?? "—"}</span>
    {#if scale !== 1}
      <span class="dim">
        {t("board.unit", { scale: scale.toLocaleString("en-US"), currency: market.info?.currency ?? "" })}
      </span>
    {/if}
  </header>

  {#if error}
    <p class="error" role="alert">{t("tape.loadFailed", { error })}</p>
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
      ariaLabel={t("tape.gridLabel", { symbol: selected ?? "" })}
      emptyMessage={t("tape.empty", { symbol: selected ?? "" })}
    />
  </div>
</div>

<style>
  .tape {
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    grid-template-rows: auto auto minmax(0, 1fr);
    height: 100%;
    min-height: 0;
  }

  header {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    gap: 0.75rem;
    padding: 0.625rem 1rem 0.5rem;
    font-size: var(--text-sm);
  }

  .tape:global([data-narrow]) header .dim {
    display: none;
  }

  .sym {
    font-family: var(--mono);
    font-weight: 600;
    font-size: var(--text-md);
  }

  .error {
    margin: 0 1rem 0.5rem;
    padding: 0.375rem 0.625rem;
    font-size: var(--text-sm);
    color: var(--down);
    border-left: 3px solid var(--down);
  }

  /* Last row whether or not the error shows. */
  .grid {
    grid-row: 3;
    min-height: 0;
  }

  .tape :global(.bo-grid .c.ts-ceiling) {
    color: var(--tone-ceiling);
  }

  .tape :global(.bo-grid .c.ts-floor) {
    color: var(--tone-floor);
  }

  .tape :global(.bo-grid .c.ts-ref) {
    color: var(--tone-ref);
  }

  .tape :global(.bo-grid .c.ts-up) {
    color: var(--tone-up);
  }

  .tape :global(.bo-grid .c.ts-down) {
    color: var(--tone-down);
  }
</style>
