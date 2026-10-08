<script>
  // The text size, for every window at once (ADR 0014). Every size in the
  // app is rem, so this one choice scales text, spacing, grids, and panels.
  import ToolSelect from "./ToolSelect.svelte";
  import { SCALES, setScale, ui } from "./ui.svelte.js";
  import { t } from "../i18n/i18n.svelte.js";

  const percent = (scale) => `${Math.round(scale * 100)}%`;
  const options = $derived(SCALES.map((scale) => ({ value: scale, text: t("app.textSizeOption", { size: percent(scale) }) })));
</script>

<ToolSelect value={ui.scale} label={t("app.textSize")} {options} onchange={setScale} />
