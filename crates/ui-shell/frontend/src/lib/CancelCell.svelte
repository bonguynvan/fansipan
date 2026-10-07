<script>
  import { cancelOrder, cancelling } from "./orders.svelte.js";

  let { row } = $props();

  const visible = $derived(row.open && row.status !== "cancelling");
</script>

{#if visible}
  <button
    class="cancel"
    disabled={cancelling.has(row.id)}
    onclick={(event) => {
      event.stopPropagation();
      cancelOrder(row.id);
    }}
    aria-label="Cancel order {row.id}">Cancel</button
  >
{/if}

<style>
  .cancel {
    padding: 2px 10px;
    font: inherit;
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    background: transparent;
    border: 1px solid var(--rule);
    color: var(--text-dim);
    cursor: pointer;
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
