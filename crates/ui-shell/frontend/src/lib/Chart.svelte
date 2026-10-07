<script>
  import { TradeCanvas } from "@tradecanvas/svelte";
  import { BAR_TIMEFRAME, ShellAdapter } from "./shellAdapter.js";
  import { market } from "./market.svelte.js";
  import { symbolOf } from "./links.svelte.js";

  let { params } = $props();

  const symbol = $derived(symbolOf(params, market.symbols[0]));

  const adapter = new ShellAdapter();
  // Chart-side trading, alerts and screenshots stay off until they are wired
  // to the engines; a button that does nothing is worse than no button.
  const features = { trading: false, alerts: false, screenshot: false };
  const indicators = [{ id: "ema", params: { period: 20 } }];
</script>

<div class="chart">
  <header>
    <span class="sym">{symbol ?? "—"}</span>
    <span class="dim">{BAR_TIMEFRAME} bars · mid · tick volume</span>
  </header>

  <div class="canvas">
    {#if symbol}
      <TradeCanvas
        {symbol}
        timeframe={BAR_TIMEFRAME}
        theme="dark"
        {adapter}
        {features}
        {indicators}
        historyLimit={500}
      />
    {:else}
      <p class="dim empty">Waiting for the first quotes…</p>
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
    gap: 12px;
    align-items: baseline;
    padding: 10px 16px 8px;
  }


  .sym {
    font-family: var(--mono);
    font-weight: 600;
  }

  header .dim {
    margin-left: auto;
    font-size: 11px;
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
    margin: 16px;
    font-size: 12px;
  }
</style>
