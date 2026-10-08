<script>
  import { t } from "../i18n/i18n.svelte.js";

  // `column.onRemove` is the watchlist panel's: bo-grid mounts this cell and
  // hands it the column definition, which carries the panel's callback.
  let { row, column } = $props();
</script>

<!-- A key on this button is its own: the grid would otherwise take Enter
     as picking its focus row's symbol, and remove nothing. -->
<button
  class="remove"
  onkeydown={(event) => event.stopPropagation()}
  onclick={(event) => {
    event.stopPropagation();
    column.onRemove?.(row.symbol);
  }}
  aria-label={t("watchlist.remove", { symbol: row.symbol })}
  title={t("watchlist.remove", { symbol: row.symbol })}>×</button
>

<style>
  .remove {
    width: 1.375rem;
    height: 1.25rem;
    padding: 0;
    font: inherit;
    font-size: var(--text-lg);
    line-height: 1;
    background: transparent;
    border: 1px solid transparent;
    color: var(--text-dim);
    cursor: pointer;
  }

  .remove:hover,
  .remove:focus-visible {
    color: var(--down);
    border-color: var(--down);
    outline: none;
  }
</style>
