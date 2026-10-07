<script>
  import { invoke } from "@tauri-apps/api/core";
  import { market } from "./market.svelte.js";
  import { symbolOf } from "./links.svelte.js";
  import { money, price, quantity as formatQuantity } from "./format.js";
  import { stepDown, stepUp, tickTable } from "./ticks.js";
  import { t } from "../i18n/i18n.svelte.js";

  let { params } = $props();

  const selected = $derived(symbolOf(params, market.symbols[0]));
  const quote = $derived(selected ? market.quotes[selected] : null);
  // The instrument's rules, so the inputs offer what the venue accepts.
  // Risk checks them again in exact decimals; these only help.
  const instrument = $derived(selected ? market.instruments[selected] : null);
  const band = $derived(selected ? market.boards[selected] : null);
  const lot = $derived(Number(instrument?.lot_size) || 1);
  const ticks = $derived(tickTable(instrument));
  const decimals = $derived(market.priceDecimals(selected));

  // What was typed starts over when the symbol changes: a size or a price
  // meant for one instrument is wrong for the next, whose lots and prices
  // differ. Assigning to these overrides them until then.
  const blankFor = (_symbol) => null;
  let side = $state("buy");
  /** What was typed; until then, one lot. */
  let typedQuantity = $derived(blankFor(selected));
  const quantity = $derived(typedQuantity ?? lot);
  let orderType = $state("market");
  let limitPrice = $derived(blankFor(selected));
  let message = $state(null);
  let busy = $state(false);

  const reference = $derived(quote ? Number(side === "buy" ? quote.ask : quote.bid) : null);
  // A limit order is valued at its limit, which is also what risk checks.
  const valuedAt = $derived(orderType === "limit" && Number(limitPrice) > 0 ? Number(limitPrice) : reference);
  const notional = $derived(valuedAt && quantity > 0 ? valuedAt * quantity : null);

  const REFUSED = new Set(["denied", "rejected"]);

  /** Number inputs bind to null when empty; send the raw text and let Rust validate. */
  const asText = (value) => (value === null || value === undefined ? "" : String(value));

  function stepLimit(step) {
    limitPrice = step(ticks, Number(limitPrice) || reference || 0);
  }

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
        ? { kind: "err", text: t(order.status === "denied" ? "ticket.denied" : "ticket.rejected", { reason: order.reason }) }
        : {
            kind: "ok",
            text: t("ticket.sent", {
              id: order.id,
              side: t(`side.${side}`),
              quantity: formatQuantity(quantity),
              symbol: selected,
            }),
          };
    } catch (err) {
      message = { kind: "err", text: String(err) };
    } finally {
      busy = false;
    }
  }
</script>

<div class="ticket">
  <header>
    <span class="sym">{selected ?? "—"}</span>
  </header>

  <div class="sides" role="group" aria-label={t("ticket.side")}>
    <button class="side buy" class:active={side === "buy"} onclick={() => (side = "buy")}>{t("side.buy")}</button>
    <button class="side sell" class:active={side === "sell"} onclick={() => (side = "sell")}>{t("side.sell")}</button>
  </div>

  <form onsubmit={submit}>
    <label>
      <span>
        {t("ticket.quantity")}
        {#if lot > 1}<em class="hint">{t("ticket.lot", { lot: formatQuantity(lot) })}</em>{/if}
      </span>
      <input
        class="num"
        type="number"
        min={lot}
        step={lot}
        bind:value={() => quantity, (value) => (typedQuantity = value)}
      />
    </label>

    <label>
      <span>{t("ticket.type")}</span>
      <select bind:value={orderType} onchange={onTypeChange}>
        <option value="market">{t("ticket.market")}</option>
        <option value="limit">{t("ticket.limit")}</option>
      </select>
    </label>

    {#if orderType === "limit"}
      <label>
        <span>
          {t("ticket.limitPrice")}
          {#if band?.ceiling && band?.floor}
            <em class="hint">
              {t("ticket.band", { floor: price(band.floor, decimals), ceiling: price(band.ceiling, decimals) })}
            </em>
          {/if}
        </span>
        <span class="stepper">
          <!-- step="any": the browser measures steps from 0, which a ladder
               whose tiers start off a tick multiple would fail; risk checks
               the tick and says why. -->
          <input class="num" type="number" min="0" step="any" bind:value={limitPrice} />
          <button type="button" aria-label={t("ticket.tickDown")} title={t("ticket.tickDown")} onclick={() => stepLimit(stepDown)}
            >−</button
          >
          <button type="button" aria-label={t("ticket.tickUp")} title={t("ticket.tickUp")} onclick={() => stepLimit(stepUp)}
            >+</button
          >
        </span>
      </label>
    {/if}

    <dl class="summary">
      <div>
        <dt class="dim">{t("ticket.reference")}</dt>
        <dd class="num">{price(reference, decimals)}</dd>
      </div>
      <div>
        <dt class="dim">{t("ticket.notional")}</dt>
        <dd class="num">{money(notional, market.info?.currency)}</dd>
      </div>
    </dl>

    <button class="submit {side}" type="submit" disabled={!selected || busy}>
      {busy ? t("ticket.sending") : t("ticket.submit", { side: t(`side.${side}`), symbol: selected ?? "" })}
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

  .hint {
    margin-left: 6px;
    font-style: normal;
    text-transform: none;
    letter-spacing: 0;
  }

  .stepper {
    display: grid;
    grid-template-columns: 1fr auto auto;
  }

  .stepper button {
    width: 34px;
    border: 1px solid var(--rule);
    border-left: none;
    background: var(--panel-raised);
    color: var(--text-dim);
    font-size: 15px;
    cursor: pointer;
  }

  .stepper button:hover,
  .stepper button:focus-visible {
    color: var(--text);
    outline: none;
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
