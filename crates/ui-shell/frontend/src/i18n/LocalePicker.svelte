<script>
  // The interface language, for every window at once. Lists whatever
  // catalogs src/i18n/locales holds, by each language's own name.
  import ToolSelect from "../lib/ToolSelect.svelte";
  import { availableLocales, i18n, setLocale, t } from "./i18n.svelte.js";

  const options = availableLocales().map((locale) => ({ value: locale.id, text: locale.name }));
</script>

<ToolSelect value={i18n.locale} label={t("app.language")} {options} onchange={setLocale} />
