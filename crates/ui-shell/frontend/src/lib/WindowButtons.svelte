<script>
  // The main window has no system frame (ADR 0017), so it draws its own
  // minimise, maximise or restore, and close buttons, at the end of its
  // title bar, where Windows keeps them.
  import { onMount } from "svelte";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { t } from "../i18n/i18n.svelte.js";

  const appWindow = getCurrentWindow();
  let maximized = $state(false);

  const read = () =>
    appWindow.isMaximized().then(
      (value) => (maximized = value),
      () => {},
    );

  onMount(() => {
    read();
    const resized = appWindow.onResized(read);
    return () => resized.then((unlisten) => unlisten());
  });

  // A button that cannot act (no window to act on, as in a browser preview)
  // does nothing rather than throw.
  const act = (run) => () => run().catch(() => {});
</script>

<div class="buttons">
  <button class="minimize" aria-label={t("window.minimize")} title={t("window.minimize")} onclick={act(() => appWindow.minimize())}>
    <svg viewBox="0 0 10 10" aria-hidden="true"><path d="M1 5.5h8" /></svg>
  </button>
  <button
    class="maximize"
    aria-label={maximized ? t("window.restore") : t("window.maximize")}
    title={maximized ? t("window.restore") : t("window.maximize")}
    onclick={act(() => appWindow.toggleMaximize())}
  >
    {#if maximized}
      <svg viewBox="0 0 10 10" aria-hidden="true"><path d="M1.5 3.5h5v5h-5z M3.5 3.5v-2h5v5h-2" /></svg>
    {:else}
      <svg viewBox="0 0 10 10" aria-hidden="true"><path d="M1.5 1.5h7v7h-7z" /></svg>
    {/if}
  </button>
  <button class="close" aria-label={t("window.close")} title={t("window.close")} onclick={act(() => appWindow.close())}>
    <svg viewBox="0 0 10 10" aria-hidden="true"><path d="M1.5 1.5l7 7M8.5 1.5l-7 7" /></svg>
  </button>
</div>

<style>
  .buttons {
    display: flex;
    align-self: stretch;
  }

  button {
    display: grid;
    place-items: center;
    width: 2.875rem;
    padding: 0;
    color: var(--text-dim);
    background: transparent;
    border: none;
    cursor: pointer;
  }

  svg {
    width: 0.625rem;
    height: 0.625rem;
    fill: none;
    stroke: currentColor;
    stroke-width: 1;
  }

  button:hover,
  button:focus-visible {
    color: var(--text);
    background: color-mix(in srgb, var(--text) 10%, transparent);
    outline: none;
  }

  /* Windows' own close button turns red under the pointer. */
  .close:hover,
  .close:focus-visible {
    color: var(--on-down);
    background: var(--down);
  }
</style>
