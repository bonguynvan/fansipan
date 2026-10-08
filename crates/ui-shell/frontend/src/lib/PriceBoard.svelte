<script>
  // A price board in the APAC convention: three levels a side around the
  // last match, each price coloured by where it sits against the day's
  // reference, ceiling and floor. Bands and ticks come from the adapter;
  // this screen only resolves tones and lays them out.
  import { Grid, tradecanvasTheme } from "bo-grid";
  import { darkToneColors, toneColor } from "bo-grid/trading";
  import { market } from "./market.svelte.js";
  import { chooseSymbol, symbolOf } from "./links.svelte.js";
  import { marketPreset } from "./markets.js";
  import { boardRows, exchangesOf } from "./board.js";
  import { tickDecimals, tickTable } from "./ticks.js";
  import { phaseName } from "./orderTypes.js";
  import { t } from "../i18n/i18n.svelte.js";

  let { params, panel } = $props();

  const TONES = ["ceiling", "floor", "ref", "up", "down"];
  const PRICE_WIDTH = 58;
  const SIZE_WIDTH = 70;

  const toneVars = TONES.map((name) => `--pb-${name}: ${toneColor(name, darkToneColors)}`).join("; ");

  const preset = $derived(marketPreset(market.info?.id));
  const scale = $derived(preset.priceScale);
  const exchange = $derived(params.exchange ?? "");
  const exchanges = $derived(exchangesOf(market.instruments));
  const selected = $derived(symbolOf(params, null));

  const rows = $derived(
    boardRows(market.boards, { exchange, scale }).map((row) => ({
      ...row,
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
    type: "price", key, header, width: PRICE_WIDTH, format: formatPrice, sortable: false, autoWidth: true, ...extra,
  });
  const size = (key, header, extra = {}) => ({
    type: "volume", key, header, width: SIZE_WIDTH, format: formatSize, sortable: false, autoWidth: true, ...extra,
  });

  /** Price and size per level; bids run 3-2-1 and asks 1-2-3 so the best of each meets the match. */
  function levels(side, group, order) {
    return order.flatMap((n) => [
      price(`${side}${n}`, t("board.priceN", { n }), { group, cellClass: toneOf(`${side}${n}`), ...live }),
      size(`${side}${n}v`, t("board.volumeN", { n }), { group, cellClass: toneOf(`${side}${n}`), ...live }),
    ]);
  }

  const columns = $derived([
    { type: "text", key: "symbol", header: t("column.symbol"), width: 64, pinned: "left", cellClass: toneOf("last") },
    price("ref", t("board.ref"), { cellClass: "pb-ref" }),
    price("ceiling", t("board.ceiling"), { cellClass: "pb-ceiling" }),
    price("floor", t("board.floor"), { cellClass: "pb-floor" }),
    ...levels("b", t("board.bids"), [3, 2, 1]),
    price("last", t("board.price"), { group: t("board.matched"), cellClass: toneOf("last"), sortable: true, ...live }),
    size("lastv", t("board.volume"), { group: t("board.matched"), cellClass: toneOf("last") }),
    price("change", t("board.change"), { group: t("board.matched"), format: formatChange, cellClass: toneOf("last") }),
    {
      type: "number", key: "pct", header: t("board.pct"), width: 56, group: t("board.matched"),
      format: formatPct, cellClass: toneOf("last"),
    },
    ...levels("a", t("board.asks"), [1, 2, 3]),
    size("volume", t("board.totalVolume"), { group: t("board.day"), width: 92, sortable: true, flash: "change" }),
    price("high", t("board.high"), { group: t("board.day"), cellClass: toneOf("high") }),
    price("low", t("board.low"), { group: t("board.day"), cellClass: toneOf("low") }),
  ]);
</script>

<div class="board" style={toneVars}>
  <header>
    <div class="chips" role="group" aria-label={t("board.exchange")}>
      <button class:on={!exchange} aria-pressed={!exchange} onclick={() => panel?.updateParameters({ exchange: "" })}>
        {t("board.all")}
      </button>
      {#each exchanges as code (code)}
        <button
          class:on={exchange === code}
          aria-pressed={exchange === code}
          onclick={() => panel?.updateParameters({ exchange: code })}>{code}</button
        >
      {/each}
    </div>

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
        <li><i style:background={`var(--pb-${name})`}></i>{t(`board.tone.${name}`)}</li>
      {/each}
    </ul>
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
    gap: 8px 14px;
    padding: 8px 12px;
    font-size: 11px;
  }

  .chips {
    display: flex;
    gap: 2px;
  }

  .chips button {
    padding: 3px 10px;
    font: 600 11px/1.4 var(--mono);
    letter-spacing: 0.04em;
    background: transparent;
    border: 1px solid var(--rule);
    color: var(--text-dim);
    cursor: pointer;
  }

  .chips button:hover,
  .chips button:focus-visible {
    color: var(--text);
    border-color: var(--text-dim);
    outline: none;
  }

  .chips button.on {
    color: var(--ink);
    background: var(--accent);
    border-color: var(--accent);
  }

  .session {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 2px 9px;
    border: 1px solid var(--rule);
    border-radius: 999px;
    color: var(--text);
  }

  .dot {
    width: 7px;
    height: 7px;
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
    gap: 12px;
    margin: 0 0 0 auto;
    padding: 0;
    list-style: none;
    color: var(--text-dim);
  }

  .legend li {
    display: inline-flex;
    align-items: center;
    gap: 5px;
  }

  .legend i {
    width: 8px;
    height: 8px;
    border-radius: 50%;
  }

  .grid {
    min-height: 0;
  }

  .board :global(.bo-grid .c.pb-ceiling) {
    color: var(--pb-ceiling);
  }

  .board :global(.bo-grid .c.pb-floor) {
    color: var(--pb-floor);
  }

  .board :global(.bo-grid .c.pb-ref) {
    color: var(--pb-ref);
  }

  .board :global(.bo-grid .c.pb-up) {
    color: var(--pb-up);
  }

  .board :global(.bo-grid .c.pb-down) {
    color: var(--pb-down);
  }
</style>
