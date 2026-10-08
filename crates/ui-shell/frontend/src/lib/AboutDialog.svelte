<script>
  // Help › About Fansipan: what the app is, its version, and its licence.
  import { getVersion } from "@tauri-apps/api/app";
  import { t } from "../i18n/i18n.svelte.js";

  let { open = $bindable(false) } = $props();

  let dialog;
  let version = $state("");

  $effect(() => {
    if (!dialog) return;
    if (open && !dialog.open) {
      dialog.showModal();
      getVersion().then(
        (v) => (version = v),
        () => (version = ""),
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
