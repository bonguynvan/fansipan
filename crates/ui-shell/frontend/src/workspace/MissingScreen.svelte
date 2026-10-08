<script>
  // Shown when a saved layout names a screen nobody registered, e.g. one
  // from a plugin that has since been removed. The rest of the layout loads.
  import { t } from "../i18n/i18n.svelte.js";

  let { name } = $props();
</script>

<div class="missing">
  <p>{t("workspace.missingScreen", { name })}</p>
  <p class="dim">{t("workspace.missingScreenHint")}</p>
</div>

<style>
  .missing {
    padding: 1rem;
    font-size: var(--text-sm);
  }

  p {
    margin: 0 0 0.375rem;
  }

</style>
