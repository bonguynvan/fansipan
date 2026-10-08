<script>
  // The colour theme, for every window at once (ADR 0015).
  import ToolSelect from "./ToolSelect.svelte";
  import { THEMES, setTheme, theme } from "./theme.svelte.js";
  import { t } from "../i18n/i18n.svelte.js";

  const options = $derived(THEMES.map((choice) => ({ value: choice, text: t(`app.themeOption.${choice}`) })));
</script>

<ToolSelect value={theme.choice} label={t("app.theme")} {options} onchange={setTheme} />
