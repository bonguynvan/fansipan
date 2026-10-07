<script>
  import { invoke } from "@tauri-apps/api/core";

  let { selected, quote } = $props();

  let side = $state("buy");
  let quantity = $state(10);
  let orderType = $state("market");
  let limitPrice = $state(null);
  let message = $state(null);
  let busy = $state(false);

  const reference = $derived(quote ? Number(side === "buy" ? quote.ask : quote.bid) : null);
  // A limit order is valued at its limit, which is also what risk checks.
  const valuedAt = $derived(orderType === "limit" && Number(limitPrice) > 0 ? Number(limitPrice) : reference);
  const notional = $derived(valuedAt && quantity > 0 ? valuedAt * quantity : null);

  const REFUSED = new Set(["denied", "rejected"]);

  /** Number inputs bind to null when empty; send the raw text and let Rust validate. */
  const asText = (value) => (value === null || value === undefined ? "" : String(value));

  function onTypeChange() {
    if (orderType === "limit" && (limitPrice === null || limitPrice === "") && reference) {
      limitPrice = reference;
    }
  }

  async function submit(event) {
    event.preventDefault();
    if (!selected) return;
    busy = true;
    message = null;
    try {
      const order = await invoke("place_order", {
        symbol: selected,
        side,
        quantity: asText(quantity),
        limitPrice: orderType === "limit" ? asText(limitPrice) : null,
      });
      message = REFUSED.has(order.status)
        ? { kind: "err", text: `${order.status === "denied" ? "Denied by risk" : "Rejected"}: ${order.reason}` }
        : { kind: "ok", text: `#${order.id} ${side.toUpperCase()} ${quantity} ${selected} sent; live status in Orders` };
    } catch (err) {
      message = { kind: "err", text: String(err) };
    } finally {
      busy = false;
    }
  }
</script>

<div class="ticket">
  <header>
    <h2>Order ticket</h2>
    <span class="sym">{selected ?? "—"}</span>
  </header>

  <div class="sides" role="group" aria-label="Side">
    <button class="side buy" class:active={side === "buy"} onclick={() => (side = "buy")}>Buy</button>
    <button class="side sell" class:active={side === "sell"} onclick={() => (side = "sell")}>Sell</button>
  </div>

  <form onsubmit={submit}>
    <label>
      <span>Quantity</span>
      <input class="num" type="number" min="1" step="1" bind:value={quantity} />
    </label>

    <label>
      <span>Type</span>
      <select bind:value={orderType} onchange={onTypeChange}>
        <option value="market">Market</option>
        <option value="limit">Limit</option>
      </select>
    </label>

    {#if orderType === "limit"}
      <label>
        <span>Limit price</span>
        <input class="num" type="number" min="0.01" step="0.01" bind:value={limitPrice} />
      </label>
    {/if}

    <dl class="summary">
      <div>
        <dt class="dim">Reference</dt>
        <dd class="num">{reference ? reference.toFixed(2) : "—"}</dd>
      </div>
      <div>
        <dt class="dim">Est. notional</dt>
        <dd class="num">{notional ? notional.toLocaleString("en-US", { maximumFractionDigits: 2 }) : "—"}</dd>
      </div>
    </dl>

    <button class="submit {side}" type="submit" disabled={!selected || busy}>
      {busy ? "Sending…" : `${side === "buy" ? "Buy" : "Sell"} ${selected ?? ""}`}
    </button>

    {#if message}
      <p class="msg {message.kind}" role="status">{message.text}</p>
    {/if}
  </form>
</div>

<style>
  .ticket {
    padding: 10px 16px 14px;
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

  .sym {
    font-family: var(--mono);
    font-weight: 600;
  }

  .sides {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 6px;
    margin-bottom: 12px;
  }

  .side {
    padding: 8px;
    border: 1px solid var(--rule);
    background: var(--panel-raised);
    cursor: pointer;
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
  }

  .side.buy.active {
    border-color: var(--up);
    color: var(--up);
  }

  .side.sell.active {
    border-color: var(--down);
    color: var(--down);
  }

  form {
    display: grid;
    gap: 10px;
  }

  label {
    display: grid;
    gap: 4px;
    font-size: 11px;
    color: var(--text-dim);
    text-transform: uppercase;
    letter-spacing: 0.1em;
  }

  input,
  select {
    padding: 8px 10px;
    background: var(--ink);
    border: 1px solid var(--rule);
    border-radius: 0;
    font-size: 14px;
  }

  input:focus,
  select:focus {
    outline: none;
    border-color: var(--accent);
  }

  .summary {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 8px;
    margin: 0;
  }

  .summary div {
    background: var(--panel-raised);
    padding: 8px 10px;
  }

  dt {
    font-size: 10px;
    text-transform: uppercase;
    letter-spacing: 0.1em;
  }

  dd {
    margin: 2px 0 0;
    font-size: 15px;
  }

  .submit {
    padding: 10px;
    border: none;
    font-weight: 700;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    cursor: pointer;
    color: var(--ink);
  }

  .submit.buy {
    background: var(--up);
  }

  .submit.sell {
    background: var(--down);
  }

  .submit:disabled {
    opacity: 0.5;
    cursor: default;
  }

  .msg {
    margin: 0;
    padding: 8px 10px;
    font-size: 12px;
    border-left: 3px solid;
  }

  .msg.ok {
    border-color: var(--up);
    color: var(--up);
  }

  .msg.err {
    border-color: var(--down);
    color: var(--down);
  }
</style>
