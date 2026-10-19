<script>
  // A price board in the APAC convention: three levels a side around the
  // last match, each price coloured by where it sits against the day's
  // reference, ceiling and floor. Bands and ticks come from the adapter;
  // this screen only resolves tones and lays them out.
  import { keyboardRow } from "./keyboardRow.js";
  import Chips from "./Chips.svelte";
  import { Grid } from "bo-grid";
  import { ROW_HEIGHT_PX, gridTheme } from "./gridTheme.js";
  import { px } from "./ui.svelte.js";
  import { market } from "./market.svelte.js";
  import { chooseSymbol, symbolOf } from "./links.svelte.js";
  import { marketPreset } from "./markets.js";
  import { boardRows, exchangesOf } from "./board.js";
  import { boardMark } from "./statuses.js";
  import { tickDecimals, tickTable } from "./ticks.js";
  import { phaseName } from "./orderTypes.js";
  import { panelSize } from "./panelSize.svelte.js";
  import { t } from "../i18n/i18n.svelte.js";

  let { params, panel } = $props();

  const TONES = ["ceiling", "floor", "ref", "up", "down"];
  const PRICE_WIDTH = 58;
  const SIZE_WIDTH = 70;


  const preset = $derived(marketPreset(market.info?.id));
  const scale = $derived(preset.priceScale);
  const exchange = $derived(params.exchange ?? "");
  const exchanges = $derived(exchangesOf(market.instruments));
  const exchangeOptions = $derived([
    { value: "", text: t("board.all") },
    ...exchanges.map((code) => ({ value: code, text: code })),
  ]);
  const selected = $derived(symbolOf(params, null, market.listed));

  const rows = $derived(
    boardRows(market.boards, { exchange, scale }).map((row) => ({
      ...row,
      mark: boardMark(market.statuses[row.symbol]),
      dp: preset.priceDecimals ?? tickDecimals(tickTable(market.instruments[row.symbol]), scale),
    })),
  );

  // Each shown market's phase of the day, as the adapter reports it: the
  // filtered exchange's, or every market's on "All".
  const sessions = $derived(
    exchange
      ? [market.sessions[exchange]].filter(Boolean)
      : exchanges.map((code) => market.sessions[code]).filter(Boolean),
  );

  /** Linked: every linked panel follows. Not linked: only this panel remembers it. */
  const choose = (symbol) => chooseSymbol(params, panel, symbol);

  const blank = (value) => value === null || value === undefined;
  const fixed = (value, places) =>
    value.toLocaleString("en-US", { minimumFractionDigits: places, maximumFractionDigits: places });
  const signed = (value, places) => `${value > 0 ? "+" : ""}${fixed(value, places)}`;
  const formatPrice = (value, row) => (blank(value) ? "" : fixed(value, row?.dp ?? 2));
  const formatChange = (value, row) => (blank(value) ? "" : signed(value, row?.dp ?? 2));
  const formatPct = (value) => (blank(value) ? "" : signed(value, 2));
  const formatSize = (value) => (blank(value) ? "" : value.toLocaleString("en-US"));

  /** A cell coloured like the price under `key` in the same row. */
  const toneOf = (key) => (_value, row) => (row.tones[key] ? `pb-${row.tones[key]}` : undefined);
  // Toned columns colour their own text, so a flash only lights the background.
  const live = { flash: "auto", flashColor: false };

  // autoWidth: a price gaining a digit (99.90 to 100.00) widens its column
  // instead of being cut.
  const price = (key, header, extra = {}) => ({
    type: "price", key, header, width: px(PRICE_WIDTH), format: formatPrice, sortable: false, autoWidth: true, ...extra,
  });
  const size = (key, header, extra = {}) => ({
    type: "volume", key, header, width: px(SIZE_WIDTH), format: formatSize, sortable: false, autoWidth: true, ...extra,
  });

  /** Price and size per level; bids run 3-2-1 and asks 1-2-3 so the best of each meets the match. */
  function levels(side, group, order) {
    return order.flatMap((n) => [
      price(`${side}${n}`, t("board.priceN", { n }), { group, cellClass: toneOf(`${side}${n}`), ...live }),
      size(`${side}${n}v`, t("board.volumeN", { n }), { group, cellClass: toneOf(`${side}${n}`), ...live }),
    ]);
  }

  const columns = $derived([
    {
      type: "text",
      key: "symbol",
      header: t("column.symbol"),
      width: px(64),
      // Grows to a status mark's words rather than cut them off.
      autoWidth: true,
      pinned: "left",
      format: (value, row) => (row.mark ? `${value} · ${t(row.mark)}` : value),
      cellClass: (value, row) => (row.mark ? "pb-stopped" : toneOf("last")(value, row)),
    },
    price("ref", t("board.ref"), { cellClass: "pb-ref" }),
    price("ceiling", t("board.ceiling"), { cellClass: "pb-ceiling" }),
    price("floor", t("board.floor"), { cellClass: "pb-floor" }),
    ...levels("b", t("board.bids"), [3, 2, 1]),
    price("last", t("board.price"), { group: t("board.matched"), cellClass: toneOf("last"), sortable: true, ...live }),
    size("lastv", t("board.volume"), { group: t("board.matched"), cellClass: toneOf("last") }),
    price("change", t("board.change"), { group: t("board.matched"), format: formatChange, cellClass: toneOf("last") }),
    {
      type: "number", key: "pct", header: t("board.pct"), width: px(56), group: t("board.matched"),
      format: formatPct, cellClass: toneOf("last"),
    },
    ...levels("a", t("board.asks"), [1, 2, 3]),
    size("volume", t("board.totalVolume"), { group: t("board.day"), width: px(92), sortable: true, flash: "change" }),
    price("high", t("board.high"), { group: t("board.day"), cellClass: toneOf("high") }),
    price("low", t("board.low"), { group: t("board.day"), cellClass: toneOf("low") }),
  ]);
</script>

<div class="board" use:panelSize>
  <header>
    <Chips
      label={t("board.exchange")}
      options={exchangeOptions}
      value={exchange}
      onchange={(code) => panel?.updateParameters({ exchange: code })}
    />

    {#each sessions as session (session.market)}
      <span class="session session-{session.phase}" title={t("board.sessionHint")}>
        <span class="dot"></span>{exchange ? "" : `${session.market} `}{phaseName(session, t)}
      </span>
    {/each}

    {#if scale !== 1}
      <span class="dim unit">
        {t("board.unit", { scale: scale.toLocaleString("en-US"), currency: market.info?.currency ?? "" })}
      </span>
    {/if}

    <ul class="legend">
      {#each TONES as name (name)}
        <li><i style:background={`var(--tone-${name})`}></i>{t(`board.tone.${name}`)}</li>
      {/each}
    </ul>
  </header>

  <div class="grid" use:keyboardRow>
    <Grid
      {rows}
      {columns}
      height="100%"
      theme={gridTheme}
      rowHeight={px(ROW_HEIGHT_PX)}
      fitColumns
      cellSelection={false}
      selectedRowId={selected}
      onRowClick={(row) => choose(row.symbol)}
      emptyMessage={t("board.waiting")}
    />
  </div>
</div>

<style>
  .board {
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    grid-template-rows: auto 1fr;
    height: 100%;
    min-height: 0;
  }

  header {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.5rem 0.875rem;
    padding: 0.5rem 0.75rem;
    font-size: var(--text-xs);
  }

  /* A compact board keeps the exchanges and sessions, and drops the legend and unit. */
  .board:global([data-compact]) .legend,
  .board:global([data-compact]) .unit {
    display: none;
  }

  .session {
    display: inline-flex;
    align-items: center;
    gap: 0.375rem;
    padding: 0.125rem 0.5625rem;
    border: 1px solid var(--rule);
    border-radius: 999rem;
    color: var(--text);
  }

  .dot {
    width: 0.4375rem;
    height: 0.4375rem;
    border-radius: 50%;
    background: var(--text-dim);
  }

  .session-continuous .dot {
    background: var(--up);
    box-shadow: 0 0 6px var(--up);
  }

  .session-openingAuction .dot,
  .session-closingAuction .dot,
  .session-afterClose .dot {
    background: var(--accent);
    box-shadow: 0 0 6px var(--accent);
  }

  .legend {
    display: flex;
    gap: 0.75rem;
    margin: 0 0 0 auto;
    padding: 0;
    list-style: none;
    color: var(--text-dim);
  }

  .legend li {
    display: inline-flex;
    align-items: center;
    gap: 0.3125rem;
  }

  .legend i {
    width: 0.5rem;
    height: 0.5rem;
    border-radius: 50%;
  }

  .grid {
    min-height: 0;
  }

  .board :global(.bo-grid .c.pb-ceiling) {
    color: var(--tone-ceiling);
  }

  .board :global(.bo-grid .c.pb-floor) {
    color: var(--tone-floor);
  }

  /* An instrument that does not trade now, or trades restricted (ADR 0026). */
  .board :global(.bo-grid .c.pb-stopped) {
    color: var(--text-dim);
    font-style: italic;
  }
  .board :global(.bo-grid .c.pb-ref) {
    color: var(--tone-ref);
  }

  .board :global(.bo-grid .c.pb-up) {
    color: var(--tone-up);
  }

  .board :global(.bo-grid .c.pb-down) {
    color: var(--tone-down);
  }
</style>
