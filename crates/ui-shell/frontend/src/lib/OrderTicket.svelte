<script>
  import { tick, untrack } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { market } from "./market.svelte.js";
  import { symbolOf } from "./links.svelte.js";
  import { picks, pickToApply } from "./picks.svelte.js";
  import { registerTicket, useTicket } from "./tickets.svelte.js";
  import { keyOf } from "./hotkeys.svelte.js";
  import { money, price, quantity as formatQuantity } from "./format.js";
  import { stepDown, stepUp, tickTable } from "./ticks.js";
  import { isPriced, orderTypesFor, pickOrderType, typeKey } from "./sessions.js";
  import { orderTypeName, phaseName } from "./orderTypes.js";
  import { UNTYPED, isSendable, typedOr } from "./ticket.js";
  import { capacityArgs } from "./capacity.js";
  import { panelSize } from "./panelSize.svelte.js";
  import { t } from "../i18n/i18n.svelte.js";

  let { params, panel } = $props();
  const id = $props.id();

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

  // A price picked on a linked screen (a ladder row) fills a linked
  // ticket's limit, and the side when the pick names one. A pick made
  // before this ticket opened is not news to it.
  let appliedPick = untrack(() => picks.latest?.id ?? null);
  $effect(() => {
    const pick = pickToApply(picks.latest, params, selected, appliedPick);
    if (!pick) return;
    appliedPick = pick.id;
    untrack(() => applyPick(pick));
  });

  // How a key reaches this ticket (F1, F2, F3): the side set, the panel
  // brought forward, the quantity ready to type over.
  let quantityInput = $state(null);
  const handle = {
    prepare(wanted) {
      if (wanted) side = wanted;
      panel?.setActive?.();
      // A ticket in a background tab is attached again only after its panel
      // is shown, so the focus waits for the next frame.
      tick().then(() =>
        requestAnimationFrame(() => {
          quantityInput?.focus();
          quantityInput?.select();
        }),
      );
    },
    symbol: () => selected,
  };
  $effect(() => registerTicket(handle));

  function applyPick(pick) {
    if (pick.side) side = pick.side;
    limitPrice = Number(pick.price);
    if (!priced) {
      const type = offered.find(isPriced);
      if (type) wantedType = typeKey(type);
    }
  }

  const reference = $derived(quote ? Number(side === "buy" ? quote.ask : quote.bid) : null);

  // How large one order can be (ADR 0016): core's answer for this side,
  // type, and price, asked again whenever the account or the orders move,
  // since both change what fits. A slower, older answer never overwrites a
  // newer one, and an answer to another question (another symbol, side,
  // type, or price) is cleared at once, so Max never fills the wrong size.
  let capacity = $state(null);
  let asked = 0;
  let askedFor = "";
  $effect(() => {
    const args = capacityArgs({ symbol: selected, side, orderType, priced, limitPrice });
    void market.account;
    void market.orders;
    const ask = ++asked;
    const question = JSON.stringify(args);
    if (question !== askedFor) capacity = null;
    askedFor = question;
    if (!args) return;
    invoke("order_capacity", args)
      .then((answer) => ask === asked && (capacity = answer))
      .catch(() => ask === asked && (capacity = null));
  });
  const maxQuantity = $derived(capacity?.max_quantity == null ? null : Number(capacity.max_quantity));
  const overMax = $derived(maxQuantity !== null && Number(quantity) > maxQuantity);
  /** What sets the largest order, in the trader's words. */
  const capacityNote = $derived.by(() => {
    if (!capacity) return "";
    switch (capacity.bound) {
      case "holding":
        return t("ticket.bound.holding", { quantity: formatQuantity(capacity.sellable) });
      case "order_size":
        return t("ticket.bound.order_size");
      case "order_value":
        return t("ticket.bound.order_value");
      default:
        return t(capacity.buying_power_estimated ? "ticket.bound.estimated" : "ticket.bound.buying_power", {
          amount: money(capacity.buying_power, market.info?.currency),
        });
    }
  });

  // Today's floor, reference, and ceiling, one click from the limit price.
  const BAND_PRICES = ["floor", "reference", "ceiling"];
  const bandPrices = $derived(BAND_PRICES.filter((name) => band?.[name]).map((name) => ({ name, value: band[name] })));
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

<div class="ticket" use:panelSize onfocusin={() => useTicket(handle)}>
  <header>
    <span class="sym">{selected ?? "—"}</span>
  </header>

  <div class="sides" role="group" aria-label={t("ticket.side")}>
    <button
      class="side buy"
      class:active={side === "buy"}
      aria-keyshortcuts={keyOf("ticket.buy") ?? undefined}
      title={keyOf("ticket.buy") ?? undefined}
      onclick={() => (side = "buy")}>{t("side.buy")}</button
    >
    <button
      class="side sell"
      class:active={side === "sell"}
      aria-keyshortcuts={keyOf("ticket.sell") ?? undefined}
      title={keyOf("ticket.sell") ?? undefined}
      onclick={() => (side = "sell")}>{t("side.sell")}</button
    >
  </div>

  <!-- novalidate: the quantity steps in lots, but an odd lot is a valid
       order on some markets; risk judges the quantity and says why. -->
  <form onsubmit={submit} novalidate>
    <!-- Each input is named by its heading alone: the buttons sharing its
         label (Max, the band prices) are not part of its name. -->
    <label>
      <span id="{id}-quantity">
        {t("ticket.quantity")}
        {#if lot > 1}<em class="hint">{t("ticket.lot", { lot: formatQuantity(lot) })}</em>{/if}
      </span>
      <input
        class="num"
        type="number"
        aria-labelledby="{id}-quantity"
        min={lot}
        step={lot}
        bind:this={quantityInput}
        bind:value={() => quantity, (value) => (typedQuantity = value)}
      />
      {#if capacity}
        <span class="capacity" class:over={overMax}>
          {#if maxQuantity !== null}
            <button type="button" class="max" disabled={maxQuantity <= 0} title={t("ticket.useMax")} onclick={() => (typedQuantity = maxQuantity)}
              >{side === "buy" && capacity.valued_at
                ? t("ticket.maxAt", { quantity: formatQuantity(maxQuantity), price: price(capacity.valued_at, decimals) })
                : t("ticket.max", { quantity: formatQuantity(maxQuantity) })}</button
            >
          {/if}
          <span class="note">{capacityNote}</span>
        </span>
      {/if}
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
        <span id="{id}-limit">{t("ticket.limitPrice")}</span>
        <span class="stepper">
          <!-- step="any": the browser measures steps from 0, which a ladder
               whose tiers start off a tick multiple would fail; risk checks
               the tick and says why. -->
          <input
            class="num"
            type="number"
            aria-labelledby="{id}-limit"
            min="0"
            step="any"
            placeholder={reference ?? ""}
            bind:value={limitPrice}
          />
          <button type="button" aria-label={t("ticket.tickDown")} title={t("ticket.tickDown")} onclick={() => stepLimit(stepDown)}
            >−</button
          >
          <button type="button" aria-label={t("ticket.tickUp")} title={t("ticket.tickUp")} onclick={() => stepLimit(stepUp)}
            >+</button
          >
        </span>
        {#if bandPrices.length}
          <span class="band" role="group" aria-label={t("ticket.bandPrices")}>
            {#each bandPrices as { name, value } (name)}
              <button type="button" class="band-price {name}" title={t(`ticket.use.${name}`)} onclick={() => (limitPrice = Number(value))}>
                <span>{t(`ticket.bandName.${name}`)}</span>
                <span class="num">{price(value, decimals)}</span>
              </button>
            {/each}
          </span>
        {/if}
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
    padding: 0.625rem 1rem 0.875rem;
  }

  header {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    margin-bottom: 0.625rem;
  }


  .sym {
    font-family: var(--mono);
    font-weight: 600;
  }

  .sides {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 0.375rem;
    margin-bottom: 0.75rem;
  }

  .side {
    padding: 0.5rem;
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
    gap: 0.625rem;
  }

  label {
    display: grid;
    gap: 0.25rem;
    font-size: var(--text-xs);
    color: var(--text-dim);
    text-transform: uppercase;
    letter-spacing: 0.1em;
  }

  /* Fields fill their row and may shrink below their natural width, so the
     ticket never pushes past its panel (docs/ui-rules.md, rule 3). */
  input,
  select {
    width: 100%;
    min-width: 0;
    padding: 0.5rem 0.625rem;
    background: var(--ink);
    border: 1px solid var(--rule);
    border-radius: 0;
    font-size: var(--text-lg);
  }

  input:focus,
  select:focus {
    outline: none;
    border-color: var(--accent);
  }

  .closed {
    margin: 0;
    padding: 0.5rem 0.625rem;
    border: 1px dashed var(--rule);
    color: var(--text-dim);
    text-transform: none;
    letter-spacing: 0;
    font-size: var(--text-sm);
  }

  .hint {
    margin-left: 0.375rem;
    font-style: normal;
    text-transform: none;
    letter-spacing: 0;
  }

  .stepper {
    display: grid;
    grid-template-columns: 1fr auto auto;
  }

  .stepper button {
    width: 2.125rem;
    border: 1px solid var(--rule);
    border-left: none;
    background: var(--panel-raised);
    color: var(--text-dim);
    font-size: var(--text-xl);
    cursor: pointer;
  }

  .stepper button:hover,
  .stepper button:focus-visible {
    color: var(--text);
    outline: none;
  }

  /* The largest order and what sets it, under the quantity. */
  .capacity {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: 0.25rem 0.5rem;
    text-transform: none;
    letter-spacing: 0;
    font-size: var(--text-sm);
  }

  .max {
    padding: 0.125rem 0.5rem;
    border: 1px solid var(--rule);
    background: var(--panel-raised);
    color: var(--text);
    font-family: var(--mono);
    font-size: var(--text-sm);
    cursor: pointer;
  }

  .max:hover:not(:disabled),
  .max:focus-visible {
    border-color: var(--accent);
    outline: none;
  }

  .max:disabled {
    color: var(--text-dim);
    cursor: default;
  }

  .note {
    min-width: 0;
    color: var(--text-dim);
  }

  /* More than the largest order: risk or the venue will refuse it. */
  .capacity.over .max,
  .capacity.over .note {
    color: var(--down);
  }

  /* Floor, reference, and ceiling, each in its price tone. A price on a
     button that sets the limit is never cut short: the buttons wrap first. */
  .band {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(6rem, 1fr));
    gap: 0.25rem;
  }

  .band-price {
    display: grid;
    min-width: 0;
    padding: 0.25rem 0.375rem;
    border: 1px solid var(--rule);
    border-top: 2px solid currentColor;
    background: var(--panel-raised);
    font-size: var(--text-xs);
    text-transform: none;
    letter-spacing: 0;
    cursor: pointer;
  }

  .band-price .num {
    font-size: var(--text-sm);
    overflow-wrap: anywhere;
  }

  .band-price:hover,
  .band-price:focus-visible {
    background: color-mix(in srgb, currentColor 12%, var(--panel-raised));
    outline: none;
  }

  .band-price.floor {
    color: var(--tone-floor);
  }

  .band-price.reference {
    color: var(--tone-ref);
  }

  .band-price.ceiling {
    color: var(--tone-ceiling);
  }

  .summary {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 0.5rem;
    margin: 0;
  }

  /* A compact ticket stacks its figures; a narrow one drops the hints and keeps the order. */
  .ticket:global([data-compact]) .summary {
    grid-template-columns: minmax(0, 1fr);
  }

  .ticket:global([data-narrow]) {
    padding: 0.5rem 0.625rem 0.75rem;
  }

  .ticket:global([data-narrow]) .hint {
    display: none;
  }

  .summary div {
    background: var(--panel-raised);
    padding: 0.5rem 0.625rem;
  }

  dt {
    font-size: var(--text-2xs);
    text-transform: uppercase;
    letter-spacing: 0.1em;
  }

  dd {
    margin: 0.125rem 0 0;
    font-size: var(--text-xl);
  }

  .submit {
    padding: 0.625rem;
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
    padding: 0.5rem 0.625rem;
    font-size: var(--text-sm);
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
