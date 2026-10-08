<script>
  // The text size, for every window at once (ADR 0014). Every size in the
  // app is rem, so this one choice scales text, spacing, grids, and panels.
  import { SCALES, setScale, ui } from "./ui.svelte.js";
  import { t } from "../i18n/i18n.svelte.js";

  const percent = (scale) => `${Math.round(scale * 100)}%`;
</script>

<select
  class="scale"
  value={ui.scale}
  aria-label={t("app.textSize")}
  title={t("app.textSize")}
  onchange={(event) => setScale(Number(event.currentTarget.value))}
>
  {#each SCALES as scale (scale)}
    <option value={scale}>{t("app.textSizeOption", { size: percent(scale) })}</option>
  {/each}
</select>

<style>
  .scale {
    padding: 0.1875rem 0.375rem;
    font-size: var(--text-xs);
    letter-spacing: 0.04em;
    background: transparent;
    border: 1px solid var(--rule);
    color: var(--text-dim);
    cursor: pointer;
  }

  .scale:hover,
  .scale:focus-visible {
    color: var(--text);
    border-color: var(--text-dim);
    outline: none;
  }

  option {
    background: var(--panel);
    color: var(--text);
  }
</style>
