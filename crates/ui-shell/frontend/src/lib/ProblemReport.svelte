<script>
  // How to report a problem: where the log is, what it holds, and its path
  // to copy (ADR 0027). Help › About and the Help section both show it.
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { t } from "../i18n/i18n.svelte.js";

  let logFolder = $state(null);
  /** null, "copied", or "failed": the last copy's outcome. */
  let copied = $state(null);

  onMount(() => {
    invoke("log_folder").then(
      (folder) => (logFolder = folder ?? null),
      () => (logFolder = null),
    );
  });

  async function copyFolder() {
    try {
      await navigator.clipboard.writeText(logFolder);
      copied = "copied";
    } catch {
      copied = "failed";
    }
  }
</script>

<section class="report" aria-labelledby="report-title">
  <h3 id="report-title">{t("about.reportTitle")}</h3>
  {#if logFolder}
    <p>{t("about.report")}</p>
    <div class="folder">
      <code>{logFolder}</code>
      <button onclick={copyFolder}>{copied === "copied" ? t("about.copied") : t("about.copy")}</button>
    </div>
    {#if copied === "failed"}<p class="dim" role="status">{t("about.copyFailed")}</p>{/if}
  {:else}
    <p class="dim">{t("about.noLog")}</p>
  {/if}
</section>

<style>
  h3 {
    margin: 0 0 0.25rem;
    font-size: var(--text-md);
  }

  p {
    margin: 0 0 0.625rem;
    font-size: var(--text-md);
  }

  .dim {
    color: var(--text-dim);
  }

  .folder {
    display: flex;
    gap: 0.5rem;
    align-items: center;
    min-width: 0;
  }

  code {
    flex: 1;
    min-width: 0;
    overflow-wrap: anywhere;
    font-family: var(--mono);
    font-size: var(--text-sm);
    color: var(--text-dim);
    user-select: all;
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
