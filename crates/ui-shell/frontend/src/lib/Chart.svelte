<script>
  import { TradeCanvas } from "@tradecanvas/svelte";
  import { DARK_THEME } from "@tradecanvas/chart";
  import { px } from "./ui.svelte.js";
  import { BAR_TIMEFRAME, ShellAdapter } from "./shellAdapter.js";
  import { market } from "./market.svelte.js";
  import { symbolOf } from "./links.svelte.js";
  import { t } from "../i18n/i18n.svelte.js";

  let { params } = $props();

  const symbol = $derived(symbolOf(params, market.symbols[0]));

  const adapter = new ShellAdapter((symbol) => market.instruments[symbol]);
  // Chart-side trading, alerts and screenshots stay off until they are wired
  // to the engines; a button that does nothing is worse than no button.
  const features = { trading: false, alerts: false, screenshot: false };
  const indicators = [{ id: "ema", params: { period: 20 } }];

  // The chart draws its text on a canvas, in pixels; its sizes follow the
  // trader's text size like every other size (ADR 0014).
  const theme = $derived({
    ...DARK_THEME,
    font: {
      ...DARK_THEME.font,
      sizeSmall: px(DARK_THEME.font.sizeSmall),
      sizeMedium: px(DARK_THEME.font.sizeMedium),
      sizeLarge: px(DARK_THEME.font.sizeLarge),
    },
  });

  // The instrument's details may arrive after the chart connected, in the
  // startup snapshot or a later batch.
  $effect(() => {
    const instrument = symbol ? market.instruments[symbol] : undefined;
    if (instrument) adapter.announce(instrument);
  });
</script>

<div class="chart">
  <header>
    <span class="sym">{symbol ?? "—"}</span>
    <span class="dim">{t("chart.caption", { timeframe: BAR_TIMEFRAME })}</span>
  </header>

  <div class="canvas">
    {#if symbol}
      <TradeCanvas
        {symbol}
        timeframe={BAR_TIMEFRAME}
        {theme}
        {adapter}
        {features}
        {indicators}
        historyLimit={500}
      />
    {:else}
      <p class="dim empty">{t("feed.waiting")}</p>
    {/if}
  </div>
</div>

<style>
  .chart {
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    grid-template-rows: auto 1fr;
    height: 100%;
    min-height: 0;
  }

  header {
    display: flex;
    gap: 0.75rem;
    align-items: baseline;
    padding: 0.625rem 1rem 0.5rem;
  }


  .sym {
    font-family: var(--mono);
    font-weight: 600;
  }

  header .dim {
    margin-left: auto;
    font-size: var(--text-xs);
  }

  .canvas {
    position: relative;
    min-height: 0;
  }

  .canvas > :global(div) {
    position: absolute;
    inset: 0;
  }

  .empty {
    margin: 1rem;
    font-size: var(--text-sm);
  }
</style>
