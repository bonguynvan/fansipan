<script>
  // The buttons at the right of each panel group's tab strip, for the
  // active panel: whether it follows the shared symbol, and its own symbol
  // when it does not.
  import { screenOf, setPanelSymbol, toggleLink } from "./workspace.svelte.js";
  import { isLinked, symbolOf } from "../lib/links.svelte.js";
  import { market } from "../lib/market.svelte.js";
  import { t } from "../i18n/i18n.svelte.js";

  let { group } = $props();

  let panel = $state.raw(null);
  let params = $state.raw({});

  function watchParams(target) {
    return target?.api.onDidParametersChange((next) => (params = next ?? {})) ?? { dispose() {} };
  }

  // Follow the group's active panel, and that panel's parameters.
  $effect(() => {
    const show = (target) => {
      panel = target ?? null;
      params = target?.params ?? {};
    };
    show(group.activePanel);
    let watching = watchParams(group.activePanel);
    const active = group.api.onDidActivePanelChange((event) => {
      show(event.panel);
      watching.dispose();
      watching = watchParams(event.panel);
    });
    return () => {
      active.dispose();
      watching.dispose();
    };
  });

  const linkable = $derived(screenOf(panel)?.linkable ?? false);
  const linked = $derived(isLinked(params));
  // An unlinked panel's own symbol, shown even before it has picked one, and
  // passed over as the panel passes it over when the market does not list it.
  const ownSymbol = $derived(symbolOf(params, market.symbols[0], market.listed) ?? "");
</script>

<div class="actions">
  {#if panel && linkable}
    {#if !linked}
      <select
        class="symbol"
        value={ownSymbol}
        aria-label={t("workspace.ownSymbol", { title: panel.title })}
        onchange={(event) => setPanelSymbol(panel, event.currentTarget.value)}
      >
        {#each market.symbols as symbol (symbol)}
          <option value={symbol}>{symbol}</option>
        {/each}
      </select>
    {/if}
    <button
      class="link"
      class:on={linked}
      aria-pressed={linked}
      title={linked ? t("workspace.linked") : t("workspace.unlinked")}
      aria-label={t("workspace.linkPanel", { title: panel.title })}
      onclick={() => toggleLink(panel)}><span class="dot" aria-hidden="true"></span></button
    >
  {/if}
</div>

<style>
  .actions {
    display: flex;
    align-items: center;
    gap: 0.25rem;
    height: 100%;
    padding: 0 0.375rem;
  }

  button {
    min-width: 1.375rem;
    height: 1.25rem;
    padding: 0 0.375rem;
    font: 600 var(--text-xs)/1 var(--mono);
    background: transparent;
    border: 1px solid var(--rule);
    color: var(--text-dim);
    cursor: pointer;
  }

  button:hover,
  button:focus-visible {
    color: var(--text);
    border-color: var(--text-dim);
    outline: none;
  }

  .link {
    display: grid;
    place-items: center;
  }

  /* Hollow when the panel keeps its own symbol, filled when it follows the shared one. */
  .dot {
    width: 0.5rem;
    height: 0.5rem;
    border-radius: 50%;
    border: 1.5px solid var(--text-dim);
  }

  .link.on {
    border-color: color-mix(in srgb, var(--accent) 55%, transparent);
  }

  .link.on .dot {
    background: var(--accent);
    border-color: var(--accent);
  }

  .symbol {
    height: 1.25rem;
    padding: 0 0.25rem;
    font: 600 var(--text-xs)/1 var(--mono);
    background: var(--ink);
    border: 1px solid var(--rule);
    color: var(--text);
  }

  .symbol:focus {
    outline: none;
    border-color: var(--accent);
  }
</style>
