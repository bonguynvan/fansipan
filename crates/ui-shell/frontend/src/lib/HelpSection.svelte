<script>
  // Help: what the terminal is, its keys, and how to report a
  // problem.
  import { onMount } from "svelte";
  import { getVersion } from "@tauri-apps/api/app";
  import ProblemReport from "./ProblemReport.svelte";
  import { hotkeys, keyOf } from "./hotkeys.svelte.js";
  import { market } from "./market.svelte.js";
  import { t } from "../i18n/i18n.svelte.js";

  /** Opens the keyboard shortcuts dialog the main window owns. */
  let { showShortcuts = () => {} } = $props();

  let version = $state("");

  // The keys a day uses most, as the trader has bound them now.
  const MOST_USED = [
    "app.search",
    "ticket.buy",
    "ticket.sell",
    "ticket.focus",
    "orders.cancel-all",
    "workspace.next-panel",
    "workspace.next-page",
  ];
  const mostUsed = $derived(
    MOST_USED.map((id) => ({ action: hotkeys.actions.find((a) => a.id === id), key: keyOf(id) })).filter(
      ({ action, key }) => action && key,
    ),
  );

  onMount(() => {
    getVersion().then(
      (v) => (version = v),
      () => (version = ""),
    );
  });
</script>

<div class="page">
  <h2>{t("section.help")}</h2>

  <section aria-labelledby="help-about">
    <h3 id="help-about">Fansipan</h3>
    {#if version}<p class="dim num">{t("about.version", { version })}</p>{/if}
    <p>{t("about.tagline")}</p>
    {#if market.info?.title}<p>{t("help.venue", { venue: market.info.title })}</p>{/if}
    <p class="dim">{t("about.license")}</p>
  </section>

  <section aria-labelledby="help-keys">
    <h3 id="help-keys">{t("settings.keys")}</h3>
    <p>{t("help.keys")}</p>
    {#if mostUsed.length}
      <dl>
        {#each mostUsed as { action, key } (action.id)}
          <dt>{t(action.title)}</dt>
          <dd><kbd>{key}</kbd></dd>
        {/each}
      </dl>
    {/if}
    <button onclick={showShortcuts}>{t("hotkey.action.shortcuts")}</button>
  </section>

  <section>
    <ProblemReport />
  </section>
</div>

<style>
  .page {
    height: 100%;
    overflow-y: auto;
    padding: 1.5rem 2rem;
    background: var(--panel);
  }

  h2 {
    margin: 0 0 1.25rem;
    font-size: var(--text-2xl);
    font-weight: 600;
  }

  section {
    max-width: 40rem;
    margin: 0 0 1.25rem;
    padding: 0 0 1.25rem;
    border-bottom: 1px solid var(--rule);
    min-width: 0;
  }

  h3 {
    margin: 0 0 0.375rem;
    font-size: var(--text-lg);
  }

  p {
    margin: 0 0 0.625rem;
    font-size: var(--text-md);
  }

  .dim {
    color: var(--text-dim);
  }

  .num {
    font-family: var(--mono);
    font-size: var(--text-sm);
  }

  dl {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    gap: 0.25rem 1rem;
    margin: 0 0 0.75rem;
    font-size: var(--text-md);
  }

  dt {
    min-width: 0;
  }

  dd {
    margin: 0;
  }

  kbd {
    font-family: var(--mono);
    font-size: var(--text-sm);
    color: var(--text-dim);
  }

  button {
    padding: 0.375rem 0.875rem;
    font-size: var(--text-md);
    color: var(--text);
    background: transparent;
    border: 1px solid var(--rule);
    cursor: pointer;
  }

  button:hover,
  button:focus-visible {
    border-color: var(--text-dim);
    outline: none;
  }
</style>
