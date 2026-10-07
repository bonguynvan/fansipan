<script>
  // The interface language, for every window at once. Lists whatever
  // catalogs src/i18n/locales holds, by each language's own name.
  import { availableLocales, i18n, setLocale, t } from "./i18n.svelte.js";

  const locales = availableLocales();
</script>

<select
  class="locale"
  value={i18n.locale}
  aria-label={t("app.language")}
  title={t("app.language")}
  onchange={(event) => setLocale(event.currentTarget.value)}
>
  {#each locales as locale (locale.id)}
    <option value={locale.id}>{locale.name}</option>
  {/each}
</select>

<style>
  .locale {
    padding: 3px 6px;
    font-size: 11px;
    letter-spacing: 0.04em;
    background: transparent;
    border: 1px solid var(--rule);
    color: var(--text-dim);
    cursor: pointer;
  }

  .locale:hover,
  .locale:focus-visible {
    color: var(--text);
    border-color: var(--text-dim);
    outline: none;
  }

  option {
    background: var(--panel);
    color: var(--text);
  }
</style>
