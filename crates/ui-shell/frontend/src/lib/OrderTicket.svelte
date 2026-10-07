<script>
  import { untrack } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { market } from "./market.svelte.js";
  import { isLinked, symbolOf } from "./links.svelte.js";
  import { picks, pickToApply } from "./picks.svelte.js";
  import { money, price, quantity as formatQuantity } from "./format.js";
  import { stepDown, stepUp, tickTable } from "./ticks.js";
  import { isPriced, orderTypesFor, pickOrderType, typeKey } from "./sessions.js";
  import { orderTypeName, phaseName } from "./orderTypes.js";
  import { UNTYPED, isSendable, typedOr } from "./ticket.js";
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
  // What the instrument's market takes now, as its session says; the
  // usual two when the adapter publishes no session.
  const exchange = $derived(instrument?.exchange ?? "");
  const session = $derived(exchange ? market.sessions[exchange] : undefined);
  const offered = $derived(orderTypesFor(exchange, market.sessions));

  // What was typed starts over when the symbol changes: a size or a price
  // meant for one instrument is wrong for the next, whose lots and prices
  // differ. Assigning to these overrides them until then.
  const blankFor = (_symbol) => null;
  const untypedFor = (_symbol) => UNTYPED;
  let side = $state("buy");
  /** What was typed; until then, one lot. Cleared, it stays empty. */
  let typedQuantity = $derived(untypedFor(selected));
  const quantity = $derived(typedOr(typedQuantity, lot));
  /** The type asked for; shown while the market still takes it. */
  let wantedType = $state(null);
  const orderType = $derived(pickOrderType(offered, wantedType));
  const priced = $derived(isPriced(orderType));
  let limitPrice = $derived(blankFor(selected));
  const missingPrice = $derived(priced && !isSendable(limitPrice));
  const missingQuantity = $derived(!isSendable(quantity));

  // A type the market stopped taking is forgotten, not kept for later: when
  // the phase changes back, the ticket must not quietly offer it again.
  $effect(() => {
    if (wantedType !== null && (!orderType || typeKey(orderType) !== wantedType)) wantedType = null;
  });
  let message = $state(null);
  let busy = $state(false);

  // A price picked on a screen in this ticket's link group (a ladder row)
  // fills the limit, and the side when the pick names one. A pick made
  // before this ticket opened is not news to it.
  let appliedPick = untrack(() => (isLinked(params.link) ? picks.latest[params.link]?.id : null) ?? null);
  $effect(() => {
    const pick = pickToApply(picks.latest, params.link, selected, appliedPick);
    if (!pick) return;
    appliedPick = pick.id;
    untrack(() => applyPick(pick));
  });

  function applyPick(pick) {
    if (pick.side) side = pick.side;
    limitPrice = Number(pick.price);
    if (!priced) {
      const type = offered.find(isPriced);
      if (type) wantedType = typeKey(type);
    }
  }

  const reference = $derived(quote ? Number(side === "buy" ? quote.ask : quote.bid) : null);
  // A limit order is valued at its limit, which is also what risk checks.
  const valuedAt = $derived(priced && Number(limitPrice) > 0 ? Number(limitPrice) : reference);
  const notional = $derived(valuedAt && quantity > 0 ? valuedAt * quantity : null);

  const REFUSED = new Set(["denied", "rejected"]);

  /** Number inputs bind to null when empty; send the raw text and let Rust validate. */
  const asText = (value) => (value === null || value === undefined ? "" : String(value));

  function stepLimit(step) {
    limitPrice = step(ticks, Number(limitPrice) || reference || 0);
  }

  function chooseType(key) {
    wantedType = key;
    const chosen = offered.find((type) => typeKey(type) === key);
    if (isPriced(chosen) && (limitPrice === null || limitPrice === "") && reference) {
      limitPrice = reference;
    }
  }

  async function submit(event) {
    event.preventDefault();
    if (!selected || !orderType || missingPrice || missingQuantity) return;
    busy = true;
    message = null;
    try {
      const order = await invoke("place_order", {
        symbol: selected,
        side,
        quantity: asText(quantity),
        limitPrice: priced ? asText(limitPrice) : null,
        kind: orderType.kind,
        timeInForce: orderType.time_in_force,
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

  <!-- novalidate: the quantity steps in lots, but an odd lot is a valid
       order on some markets; risk judges the quantity and says why. -->
  <form onsubmit={submit} novalidate>
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
      <span>
        {t("ticket.type")}
        {#if session}<em class="hint">{t("ticket.phase", { market: exchange, phase: phaseName(session, t) })}</em>{/if}
      </span>
      {#if orderType}
        <select value={typeKey(orderType)} onchange={(event) => chooseType(event.currentTarget.value)}>
          {#each offered as type (typeKey(type))}
            <option value={typeKey(type)}>{orderTypeName(type, t)}</option>
          {/each}
        </select>
      {:else}
        <p class="closed" role="status">{t("ticket.noOrders", { market: exchange })}</p>
      {/if}
    </label>

    {#if priced}
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
          <input class="num" type="number" min="0" step="any" placeholder={reference ?? ""} bind:value={limitPrice} />
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

    <button class="submit {side}" type="submit" disabled={!selected || !orderType || missingPrice || missingQuantity || busy}>
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

  .closed {
    margin: 0;
    padding: 8px 10px;
    border: 1px dashed var(--rule);
    color: var(--text-dim);
    text-transform: none;
    letter-spacing: 0;
    font-size: 12px;
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
