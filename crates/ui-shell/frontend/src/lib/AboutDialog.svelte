<script>
  // Help › About Fansipan: what the app is, its version, its licence, and
  // where its log is, to attach to a problem report (ADR 0027).
  import { getVersion } from "@tauri-apps/api/app";
  import { invoke } from "@tauri-apps/api/core";
  import { t } from "../i18n/i18n.svelte.js";

  let { open = $bindable(false) } = $props();

  let dialog;
  let version = $state("");
  let logFolder = $state(null);
  /** null, "copied", or "failed": the last copy's outcome. */
  let copied = $state(null);

  async function copyFolder() {
    try {
      await navigator.clipboard.writeText(logFolder);
      copied = "copied";
    } catch {
      copied = "failed";
    }
  }

  $effect(() => {
    if (!dialog) return;
    if (open && !dialog.open) {
      dialog.showModal();
      getVersion().then(
        (v) => (version = v),
        () => (version = ""),
      );
      copied = null;
      invoke("log_folder").then(
        (folder) => (logFolder = folder ?? null),
        () => (logFolder = null),
      );
    }
    if (!open && dialog.open) dialog.close();
  });
</script>

<dialog bind:this={dialog} class="about" aria-labelledby="about-title" onclose={() => (open = false)}>
  <h2 id="about-title">Fansipan</h2>
  {#if version}<p class="num dim">{t("about.version", { version })}</p>{/if}
  <p>{t("about.tagline")}</p>
  <p class="dim">{t("about.license")}</p>
  <section class="report" aria-labelledby="about-report">
    <h3 id="about-report">{t("about.reportTitle")}</h3>
    {#if logFolder}
      <p>{t("about.report")}</p>
      <div class="folder">
        <code class="num path">{logFolder}</code>
        <button onclick={copyFolder}>{copied === "copied" ? t("about.copied") : t("about.copy")}</button>
      </div>
      {#if copied === "failed"}<p class="dim" role="status">{t("about.copyFailed")}</p>{/if}
    {:else}
      <p class="dim">{t("about.noLog")}</p>
    {/if}
  </section>
  <div class="actions">
    <button onclick={() => (open = false)}>{t("about.close")}</button>
  </div>
</dialog>

<style>
  .about {
    width: min(26rem, calc(100vw - 2rem));
    padding: 1.125rem 1.25rem 1rem;
    background: var(--panel);
    color: var(--text);
    border: 1px solid var(--rule);
    border-radius: 0;
  }

  .about::backdrop {
    background: var(--backdrop);
  }

  h2 {
    margin: 0 0 0.25rem;
    font-size: var(--text-xl);
  }

  p {
    margin: 0 0 0.625rem;
    font-size: var(--text-md);
  }

  .num {
    font-family: var(--mono);
    font-size: var(--text-sm);
  }

  .report {
    margin: 0 0 0.625rem;
    padding-top: 0.625rem;
    border-top: 1px solid var(--rule);
  }

  h3 {
    margin: 0 0 0.25rem;
    font-size: var(--text-md);
  }

  .folder {
    display: flex;
    gap: 0.5rem;
    align-items: center;
    min-width: 0;
  }

  .folder code {
    flex: 1;
    min-width: 0;
    overflow-wrap: anywhere;
    color: var(--text-dim);
    user-select: all;
  }

  .actions {
    display: flex;
    justify-content: flex-end;
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
