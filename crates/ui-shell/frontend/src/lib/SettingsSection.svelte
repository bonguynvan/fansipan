<script>
  // Settings: how the terminal looks and reads, and its keys, in
  // one place. Each choice is the one the View menu offers, so both change
  // the same setting, here and in every window.
  import { THEMES, setTheme, theme } from "./theme.svelte.js";
  import { SCALES, setScale, ui } from "./ui.svelte.js";
  import { availableLocales, i18n, setLocale, t } from "../i18n/i18n.svelte.js";

  /** Opens the keyboard shortcuts dialog the main window owns. */
  let { showShortcuts = () => {} } = $props();

  const percent = (scale) => `${Math.round(scale * 100)}%`;
</script>

<div class="page">
  <h2>{t("section.settings")}</h2>

  <fieldset>
    <legend>{t("menu.theme")}</legend>
    <div class="choices">
      {#each THEMES as choice (choice)}
        <label class:on={theme.choice === choice}>
          <input type="radio" name="theme" checked={theme.choice === choice} onchange={() => setTheme(choice)} />
          {t(`app.themeOption.${choice}`)}
        </label>
      {/each}
    </div>
  </fieldset>

  <fieldset>
    <legend>{t("menu.textSize")}</legend>
    <div class="choices">
      {#each SCALES as scale (scale)}
        <label class:on={ui.scale === scale}>
          <input type="radio" name="scale" checked={ui.scale === scale} onchange={() => setScale(scale)} />
          {percent(scale)}
        </label>
      {/each}
    </div>
  </fieldset>

  <fieldset>
    <legend>{t("menu.language")}</legend>
    <div class="choices">
      {#each availableLocales() as locale (locale.id)}
        <label class:on={i18n.locale === locale.id}>
          <input type="radio" name="locale" checked={i18n.locale === locale.id} onchange={() => setLocale(locale.id)} />
          {locale.name}
        </label>
      {/each}
    </div>
  </fieldset>

  <fieldset>
    <legend>{t("settings.keys")}</legend>
    <p class="dim">{t("settings.keysHint")}</p>
    <button onclick={showShortcuts}>{t("hotkey.action.shortcuts")}</button>
  </fieldset>
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

  fieldset {
    max-width: 40rem;
    margin: 0 0 1.25rem;
    padding: 0 0 1.25rem;
    border: none;
    border-bottom: 1px solid var(--rule);
    min-width: 0;
  }

  legend {
    margin-bottom: 0.625rem;
    padding: 0;
    font-size: var(--text-xs);
    text-transform: uppercase;
    letter-spacing: 0.12em;
    color: var(--text-dim);
  }

  .choices {
    display: flex;
    flex-wrap: wrap;
    gap: 0.5rem;
  }

  label {
    display: flex;
    gap: 0.375rem;
    align-items: center;
    padding: 0.375rem 0.75rem;
    font-size: var(--text-md);
    border: 1px solid var(--rule);
    border-radius: 0.25rem;
    cursor: pointer;
  }

  label.on {
    color: var(--accent);
    border-color: var(--accent);
    background: color-mix(in srgb, var(--accent) 10%, transparent);
  }

  label:focus-within {
    outline: 1px solid var(--accent);
  }

  input {
    accent-color: var(--accent);
    margin: 0;
  }

  p {
    margin: 0 0 0.625rem;
    font-size: var(--text-md);
  }

  .dim {
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
