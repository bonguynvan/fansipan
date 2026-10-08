<script>
  // A depth ladder for the linked symbol: every tradable price, highest
  // first, with the book's size where it has one, kept centred on the
  // spread. A click fills the linked order tickets: the
  // price, and the side too when the click is on the bid or ask column.
  // Scrolling (the wheel, or the page buttons) holds the ladder where the
  // trader put it until Recenter.
  import { Grid } from "bo-grid";
  import { gridTheme } from "./gridTheme.js";
  import { px } from "./ui.svelte.js";
  import { centeredWindow } from "bo-grid/realtime";
  import { market } from "./market.svelte.js";
  import { isLinked, symbolOf } from "./links.svelte.js";
  import { pickPrice } from "./picks.svelte.js";
  import { marketPreset } from "./markets.js";
  import { centreIndex, ladderRows } from "./ladder.js";
  import { panelSize } from "./panelSize.svelte.js";
  import { tickDecimals, tickTable } from "./ticks.js";
  import { t } from "../i18n/i18n.svelte.js";

  let { params } = $props();

  /** Row and header heights, in pixels at 100%; px() scales them. */
  const ROW_DESIGN_PX = 24;
  const HEADER_DESIGN_PX = 32;
  const MIN_VISIBLE = 5;
  /** Rows of overlap a page keeps, so the trader does not lose their place. */
  const PAGE_OVERLAP = 2;
  const TONES = ["ceiling", "floor", "ref", "up", "down"];

  const selected = $derived(symbolOf(params, market.symbols[0]));
  const instrument = $derived(selected ? market.instruments[selected] : null);
  const table = $derived(tickTable(instrument));
  const preset = $derived(marketPreset(market.info?.id));
  const scale = $derived(preset.priceScale);
  const decimals = $derived(preset.priceDecimals ?? tickDecimals(table, scale));
  const rows = $derived(selected ? ladderRows(table, market.boards[selected], scale) : []);
  const linked = $derived(isLinked(params));

  let gridHeight = $state(0);
  const rowHeight = $derived(px(ROW_DESIGN_PX));
  const visibleCount = $derived(Math.max(MIN_VISIBLE, Math.floor((gridHeight - px(HEADER_DESIGN_PX)) / rowHeight)));

  // null follows the market; `{ symbol, key }` holds the ladder on that
  // price of that symbol. A key, not an index, so the held window stays on
  // the same prices when the band, and with it the row list, changes.
  let held = $state(null);
  const heldIndex = $derived(
    held?.symbol === selected ? rows.findIndex((row) => row.key === held.key) : -1,
  );
  const centre = $derived(heldIndex === -1 ? centreIndex(rows) : heldIndex);
  const shown = $derived.by(() => {
    const { start, end } = centeredWindow(rows.length, centre, visibleCount);
    return rows.slice(start, end);
  });

  const page = $derived(Math.max(1, visibleCount - PAGE_OVERLAP));

  // A held price that left the ladder (the market drifted past it) lets
  // go, rather than taking hold again if the price drifts back.
  $effect(() => {
    if (held !== null && heldIndex === -1) held = null;
  });

  /** Moves the ladder by `count` rows: negative toward higher prices. */
  function moveBy(count) {
    if (!rows.length || count === 0) return;
    const next = Math.max(0, Math.min(rows.length - 1, centre + count));
    held = { symbol: selected, key: rows[next].key };
  }

  // A trackpad sends many small deltas and a mouse a few large ones: both
  // add up here and move the ladder a whole row at a time.
  let wheelTravel = 0;

  function onWheel(event) {
    if (!rows.length) return;
    event.preventDefault();
    const unit =
      event.deltaMode === WheelEvent.DOM_DELTA_LINE ? rowHeight : event.deltaMode === WheelEvent.DOM_DELTA_PAGE ? gridHeight : 1;
    wheelTravel += event.deltaY * unit;
    const whole = Math.trunc(wheelTravel / rowHeight);
    wheelTravel -= whole * rowHeight;
    moveBy(whole);
  }

  const SIDE_OF_COLUMN = { bid: "buy", ask: "sell" };

  let pickError = $state(null);

  function pick(row, side) {
    if (!selected) return;
    pickError = null;
    pickPrice(params, { symbol: selected, price: row.key, side }).catch((err) => (pickError = String(err)));
  }

  const fixed = (value, places) =>
    value.toLocaleString("en-US", { minimumFractionDigits: places, maximumFractionDigits: places });
  const size = (value) => (value === null || value === undefined ? "" : value.toLocaleString("en-US"));

  const columns = $derived([
    {
      type: "volume",
      key: "bid",
      header: t("ladder.bid"),
      width: px(80),
      sortable: false,
      format: size,
      cellClass: "ld-bid",
      flash: "change",
    },
    {
      type: "price",
      key: "price",
      header: t("board.price"),
      width: px(80),
      sortable: false,
      format: (value) => fixed(value, decimals),
      cellClass: (_value, row) => (row.tone ? `ld-${row.tone}` : undefined),
    },
    {
      type: "volume",
      key: "ask",
      header: t("ladder.ask"),
      width: px(80),
      sortable: false,
      // Both sides' sizes sit against the price between them.
      align: "left",
      format: size,
      cellClass: "ld-ask",
      flash: "change",
    },
  ]);

  const rowClass = (row) => [row.last && "ld-last", row.touch && "ld-touch"].filter(Boolean).join(" ");
</script>

<div class="ladder" use:panelSize>
  <header>
    <span class="sym">{selected ?? "—"}</span>
    <span class="pager" role="group" aria-label={t("ladder.scroll")}>
      <button aria-label={t("ladder.higher")} title={t("ladder.higher")} onclick={() => moveBy(-page)}>▲</button>
      <button aria-label={t("ladder.lower")} title={t("ladder.lower")} onclick={() => moveBy(page)}>▼</button>
    </span>
    {#if heldIndex !== -1}
      <button class="recenter" onclick={() => (held = null)}>{t("ladder.recenter")}</button>
    {/if}
    <span class="dim hint">{linked ? t("ladder.hint") : t("ladder.unlinked")}</span>
  </header>

  {#if pickError}
    <p class="error" role="alert">{t("ladder.pickFailed", { error: pickError })}</p>
  {/if}

  <div class="grid" bind:clientHeight={gridHeight} onwheel={onWheel}>
    <Grid
      rows={shown}
      {columns}
      height="100%"
      {rowHeight}
      theme={gridTheme}
      fitColumns
      {rowClass}
      ariaLabel={t("ladder.gridLabel", { symbol: selected ?? "" })}
      emptyMessage={t("ladder.empty", { symbol: selected ?? "" })}
      onCellClick={({ row, column }) => pick(row, SIDE_OF_COLUMN[column.key] ?? null)}
      onRowClick={(row, event) => {
        // A mouse click is handled per cell; Enter on the focused cell picks
        // its price alone.
        if (event instanceof KeyboardEvent) pick(row, null);
      }}
    />
  </div>
</div>

<style>
  .ladder {
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    grid-template-rows: auto auto minmax(0, 1fr);
    height: 100%;
    min-height: 0;
  }

  header {
    display: flex;
    align-items: baseline;
    gap: 0.625rem;
    padding: 0.625rem 1rem 0.5rem;
    font-size: var(--text-sm);
  }

  .sym {
    font-family: var(--mono);
    font-weight: 600;
    font-size: var(--text-md);
  }

  .hint {
    margin-left: auto;
    text-align: right;
  }

  .ladder:global([data-compact]) .hint {
    display: none;
  }

  .recenter {
    padding: 0.125rem 0.625rem;
    font: inherit;
    font-size: var(--text-xs);
    text-transform: uppercase;
    letter-spacing: 0.06em;
    background: transparent;
    border: 1px solid var(--accent);
    color: var(--accent);
    cursor: pointer;
  }

  .recenter:hover,
  .recenter:focus-visible {
    background: var(--accent);
    color: var(--ink);
    outline: none;
  }

  .pager {
    display: inline-flex;
  }

  .pager button {
    width: 1.625rem;
    padding: 0.0625rem 0;
    font: inherit;
    font-size: var(--text-2xs);
    background: transparent;
    border: 1px solid var(--rule);
    color: var(--text-dim);
    cursor: pointer;
  }

  .pager button + button {
    border-left: none;
  }

  .pager button:hover,
  .pager button:focus-visible {
    color: var(--text);
    outline: none;
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
    overflow: hidden;
  }

  .grid :global(.bo-grid .c) {
    cursor: pointer;
  }

  .grid :global(.bo-grid .c.ld-bid) {
    color: var(--up);
  }

  .grid :global(.bo-grid .c.ld-ask) {
    color: var(--down);
  }

  .grid :global(.bo-grid .row.ld-touch) {
    background: var(--panel-raised);
  }

  .grid :global(.bo-grid .row.ld-last .c) {
    font-weight: 700;
  }

  .grid :global(.bo-grid .row.ld-last) {
    box-shadow: inset 3px 0 0 var(--accent);
  }

  .ladder :global(.bo-grid .c.ld-ceiling) {
    color: var(--tone-ceiling);
  }

  .ladder :global(.bo-grid .c.ld-floor) {
    color: var(--tone-floor);
  }

  .ladder :global(.bo-grid .c.ld-ref) {
    color: var(--tone-ref);
  }

  .ladder :global(.bo-grid .c.ld-up) {
    color: var(--tone-up);
  }

  .ladder :global(.bo-grid .c.ld-down) {
    color: var(--tone-down);
  }
</style>
