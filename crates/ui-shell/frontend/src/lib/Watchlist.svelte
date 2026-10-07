<script>
  import { price } from "./format.js";

  let { quotes, symbols, selected = $bindable(null) } = $props();

  const FLASH_MS = 450;
  const lastBid = new Map();
  let flash = $state({});

  $effect(() => {
    for (const symbol of symbols) {
      const raw = quotes[symbol]?.bid;
      if (raw === undefined) continue;
      const bid = Number(raw);
      const previous = lastBid.get(symbol);
      lastBid.set(symbol, bid);
      if (previous === undefined || previous === bid) continue;
      flash[symbol] = bid > previous ? "up" : "down";
      setTimeout(() => {
        if (flash[symbol]) flash[symbol] = null;
      }, FLASH_MS);
    }
  });

  function spread(q) {
    return (Number(q.ask) - Number(q.bid)).toFixed(2);
  }
</script>

<div class="watch">
  <header>
    <h2>Watchlist</h2>
    <span class="dim">{symbols.length} instruments</span>
  </header>

  <table>
    <thead>
      <tr>
        <th>Symbol</th>
        <th class="r">Bid</th>
        <th class="r">Ask</th>
        <th class="r">Spread</th>
      </tr>
    </thead>
    <tbody>
      {#each symbols as symbol (symbol)}
        {@const q = quotes[symbol]}
        <tr
          class:selected={selected === symbol}
          onclick={() => (selected = symbol)}
          onkeydown={(e) => e.key === "Enter" && (selected = symbol)}
          tabindex="0"
        >
          <td class="sym">{symbol}</td>
          <td class="r num bid {flash[symbol] ?? ''}">{price(q.bid)}</td>
          <td class="r num ask {flash[symbol] ?? ''}">{price(q.ask)}</td>
          <td class="r num dim">{spread(q)}</td>
        </tr>
      {/each}
    </tbody>
  </table>

  <p class="hint dim">Click a row to load it into the order ticket.</p>
</div>

<style>
  .watch {
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
    padding: 9px 8px;
    border-bottom: 1px solid var(--rule);
    transition: background 0.4s ease-out;
  }

  .r {
    text-align: right;
  }

  tbody tr {
    cursor: pointer;
  }

  tbody tr:hover td {
    background: var(--panel-raised);
  }

  tbody tr:focus-visible {
    outline: 1px solid var(--accent);
    outline-offset: -1px;
  }

  tbody tr.selected td {
    background: color-mix(in srgb, var(--accent) 12%, transparent);
  }

  .sym {
    font-weight: 600;
    letter-spacing: 0.04em;
  }

  .bid.up,
  .ask.up {
    background: color-mix(in srgb, var(--up) 22%, transparent);
  }

  .bid.down,
  .ask.down {
    background: color-mix(in srgb, var(--down) 22%, transparent);
  }

  .hint {
    margin: 12px 0 0;
    font-size: 11px;
  }
</style>
