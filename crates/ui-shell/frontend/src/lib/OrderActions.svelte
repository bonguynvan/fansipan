<script>
  import { cancelOrder, cancelling } from "./orders.svelte.js";
  import { t } from "../i18n/i18n.svelte.js";

  // `column.onAmend` is the blotter's: each blotter keeps its own amend form.
  let { row, column } = $props();

  const cancellable = $derived(row.open && row.status !== "cancelling");
</script>

<span class="actions">
  {#if row.amendable}
    <button
      class="amend"
      data-amend={row.id}
      onclick={(event) => {
        event.stopPropagation();
        column.onAmend?.(row.id);
      }}
      aria-label={t("orders.amendOrder", { id: row.id })}>{t("orders.amend")}</button
    >
  {/if}
  {#if cancellable}
    <button
      class="cancel"
      disabled={cancelling.has(row.id)}
      onclick={(event) => {
        event.stopPropagation();
        cancelOrder(row.id);
      }}
      aria-label={t("orders.cancelOrder", { id: row.id })}>{t("orders.cancel")}</button
    >
  {/if}
</span>

<style>
  .actions {
    display: inline-flex;
    gap: 0.25rem;
  }

  button {
    padding: 0.125rem 0.625rem;
    font: inherit;
    font-size: var(--text-xs);
    text-transform: uppercase;
    letter-spacing: 0.06em;
    background: transparent;
    border: 1px solid var(--rule);
    color: var(--text-dim);
    cursor: pointer;
  }

  .amend:hover,
  .amend:focus-visible {
    color: var(--accent);
    border-color: var(--accent);
    outline: none;
  }

  .cancel:hover:not(:disabled),
  .cancel:focus-visible {
    color: var(--down);
    border-color: var(--down);
    outline: none;
  }

  .cancel:disabled {
    opacity: 0.5;
    cursor: default;
  }
</style>
