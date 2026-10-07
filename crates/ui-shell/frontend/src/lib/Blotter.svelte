<script>
  import { invoke } from "@tauri-apps/api/core";
  import { price, quantity } from "./format.js";

  let { orders } = $props();

  let cancelling = $state(new Set());
  let error = $state(null);

  const working = $derived(orders.filter((o) => o.open).length);

  async function cancel(id) {
    error = null;
    cancelling = new Set([...cancelling, id]);
    try {
      await invoke("cancel_order", { id });
    } catch (err) {
      error = `#${id}: ${String(err)}`;
    } finally {
      cancelling = new Set([...cancelling].filter((x) => x !== id));
    }
  }
</script>

<div class="blotter">
  <header>
    <h2>Orders</h2>
    <span class="dim">{working} working · {orders.length} this session</span>
  </header>

  {#if error}
    <p class="error" role="alert">{error}</p>
  {/if}

  {#if orders.length === 0}
    <p class="dim empty">No orders yet.</p>
  {:else}
    <table>
      <thead>
        <tr>
          <th>#</th>
          <th>Symbol</th>
          <th>Side</th>
          <th>Type</th>
          <th class="r">Filled</th>
          <th class="r">Avg</th>
          <th>Status</th>
          <th></th>
        </tr>
      </thead>
      <tbody>
        {#each orders as o (o.id)}
          <tr class:has-reason={o.reason}>
            <td class="num dim">{o.id}</td>
            <td class="sym">{o.symbol}</td>
            <td class={o.side === "buy" ? "up" : "down"}>{o.side.toUpperCase()}</td>
            <td class="num dim">{o.limit_price ? `LMT ${price(o.limit_price)}` : "MKT"}</td>
            <td class="r num">{quantity(o.filled_quantity)}/{quantity(o.quantity)}</td>
            <td class="r num">{price(o.avg_fill_price)}</td>
            <td><span class="pill {o.status}">{o.status}</span></td>
            <td class="r">
              {#if o.open && o.status !== "cancelling"}
                <button
                  class="cancel"
                  disabled={cancelling.has(o.id)}
                  onclick={() => cancel(o.id)}
                  aria-label="Cancel order {o.id}">Cancel</button
                >
              {/if}
            </td>
          </tr>
          {#if o.reason}
            <tr class="reason">
              <td></td>
              <td colspan="7" class="dim">{o.reason}</td>
            </tr>
          {/if}
        {/each}
      </tbody>
    </table>
  {/if}
</div>

<style>
  .blotter {
    padding: 14px 16px;
  }

  header {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    margin-bottom: 10px;
  }

  h2 {
    margin: 0;
    font-size: 12px;
    text-transform: uppercase;
    letter-spacing: 0.14em;
    color: var(--accent);
  }

  table {
    width: 100%;
    border-collapse: collapse;
  }

  th {
    text-align: left;
    font-size: 11px;
    font-weight: 500;
    color: var(--text-dim);
    padding: 6px 8px;
    border-bottom: 1px solid var(--rule);
  }

  td {
    padding: 8px;
    border-bottom: 1px solid var(--rule);
    white-space: nowrap;
  }

  tr.has-reason td {
    border-bottom: none;
  }

  .reason td {
    padding-top: 0;
    font-size: 11px;
    white-space: normal;
  }

  .r {
    text-align: right;
  }

  .sym {
    font-weight: 600;
  }

  .empty {
    margin: 8px 0;
    font-size: 12px;
  }

  .error {
    margin: 0 0 10px;
    padding: 6px 10px;
    font-size: 12px;
    color: var(--down);
    border-left: 3px solid var(--down);
  }

  .pill {
    padding: 2px 8px;
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    border: 1px solid var(--rule);
    color: var(--text-dim);
  }

  .pill.filled { color: var(--up); border-color: var(--up); }
  .pill.partial { color: var(--up); border-color: var(--up); border-style: dashed; }
  .pill.accepted, .pill.submitted { color: var(--accent); border-color: var(--accent); }
  .pill.rejected, .pill.denied { color: var(--down); border-color: var(--down); }
  .pill.cancelling { border-style: dashed; }

  .cancel {
    padding: 3px 10px;
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
