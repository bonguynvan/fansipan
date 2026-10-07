<script>
  import { mark, money, price, quantity } from "./format.js";

  let { account, quotes } = $props();

  const rows = $derived(
    account.positions.map((p) => {
      const last = mark(p, quotes[p.symbol]);
      return {
        symbol: p.symbol,
        quantity: p.quantity,
        avg: p.avg_price,
        mark: last,
        pnl: (last - Number(p.avg_price)) * Number(p.quantity),
      };
    }),
  );
</script>

<div class="portfolio">
  <header>
    <h2>Account</h2>
    <span class="num">cash {money(account.cash)}</span>
  </header>

  {#if rows.length === 0}
    <p class="dim empty">No positions. Buy something from the ticket to see it here.</p>
  {:else}
    <table>
      <thead>
        <tr>
          <th>Symbol</th>
          <th class="r">Qty</th>
          <th class="r">Avg</th>
          <th class="r">Mark</th>
          <th class="r">P&amp;L</th>
        </tr>
      </thead>
      <tbody>
        {#each rows as row (row.symbol)}
          <tr>
            <td class="sym">{row.symbol}</td>
            <td class="r num {Number(row.quantity) < 0 ? 'down' : ''}">{quantity(row.quantity)}</td>
            <td class="r num">{price(row.avg)}</td>
            <td class="r num">{price(row.mark)}</td>
            <td class="r num {row.pnl >= 0 ? 'up' : 'down'}">{money(row.pnl)}</td>
          </tr>
        {/each}
      </tbody>
    </table>
  {/if}
</div>

<style>
  .portfolio {
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
</style>
