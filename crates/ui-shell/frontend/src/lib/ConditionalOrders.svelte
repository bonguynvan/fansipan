<script>
  import { invoke } from "@tauri-apps/api/core";
  import { market } from "./market.svelte.js";
  import { symbolOf } from "./links.svelte.js";
  import { price, quantity as formatQuantity } from "./format.js";
  import { stepDown, stepUp, tickTable } from "./ticks.js";
  import { UNTYPED, typedOr } from "./ticket.js";
  import { TRIGGERS, complete, conditionWords, isLive, placeArgs } from "./conditionals.js";
  import { panelSize } from "./panelSize.svelte.js";
  import { t } from "../i18n/i18n.svelte.js";

  let { params } = $props();
  const id = $props.id();

  const selected = $derived(symbolOf(params, market.symbols[0], market.listed));
  const instrument = $derived(selected ? market.instruments[selected] : null);
  const lot = $derived(Number(instrument?.lot_size) || 1);
  const ticks = $derived(tickTable(instrument));
  const decimals = $derived(market.priceDecimals(selected));
  const last = $derived(selected ? (market.boards[selected]?.last ?? null) : null);
  // Where the market has a daily band, a marketable order is a limit at its
  // edge; elsewhere it is a market order.
  const banded = $derived(Boolean(selected && market.boards[selected]?.floor && market.boards[selected]?.ceiling));

  // Sizes and prices start over when the symbol changes, as on the ticket:
  // what was typed for one instrument is wrong for the next.
  const untypedFor = (_symbol) => UNTYPED;
  const blankFor = (_symbol) => null;
  let side = $state("sell");
  let trigger = $state("at_or_below");
  let typedQuantity = $derived(untypedFor(selected));
  const quantity = $derived(typedOr(typedQuantity, lot));
  let triggerPrice = $derived(blankFor(selected));
  let offset = $derived(blankFor(selected));
  let percent = $state(false);
  let sendAs = $state("marketable");
  let limitPrice = $derived(blankFor(selected));

  const trailing = $derived(trigger === "trailing");
  const form = $derived({ symbol: selected, side, quantity, trigger, price: triggerPrice, offset, percent, sendAs, limitPrice });
  const ready = $derived(complete(form));

  let message = $state(null);
  let busy = $state(false);

  function step(direction, value) {
    return direction(ticks, Number(value) || Number(last) || 0);
  }

  async function place(event) {
    event.preventDefault();
    if (!ready || busy) return;
    busy = true;
    message = null;
    try {
      const placed = await invoke("place_conditional", placeArgs(form));
      message = { kind: "ok", text: t("conditional.placed", { id: placed.id }) };
    } catch (err) {
      message = { kind: "err", text: String(err) };
    } finally {
      busy = false;
    }
  }

  let cancelError = $state(null);
  async function cancel(conditional) {
    cancelError = null;
    try {
      await invoke("cancel_conditional", { id: conditional.id });
    } catch (err) {
      cancelError = String(err);
    }
  }

  /** A held order's condition, its prices shown as the board shows them. */
  function condition(conditional) {
    const { id: words, vars } = conditionWords(conditional);
    const places = market.priceDecimals(conditional.symbol);
    return t(words, {
      price: price(vars.price, places),
      offset: conditional.percent ? vars.offset : price(vars.offset, places),
    });
  }

  function sentOrder(conditional) {
    const order = conditional.order_id == null ? null : market.orders[conditional.order_id];
    return order ? `#${order.id} · ${t(`status.${order.status}`)}` : `#${conditional.order_id}`;
  }
</script>

<div class="conditionals" use:panelSize>
  <header>
    <span class="sym">{selected ?? "—"}</span>
    {#if last !== null}<span class="dim num">{t("conditional.last", { price: price(last, decimals) })}</span>{/if}
  </header>

  <div class="sides" role="group" aria-label={t("ticket.side")}>
    <button class="side buy" class:active={side === "buy"} onclick={() => (side = "buy")}>{t("side.buy")}</button>
    <button class="side sell" class:active={side === "sell"} onclick={() => (side = "sell")}>{t("side.sell")}</button>
  </div>

  <!-- novalidate: the engine parses every number exactly and says what is wrong. -->
  <form onsubmit={place} oninput={() => (message = null)} novalidate>
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
        bind:value={() => quantity, (value) => (typedQuantity = value)}
      />
    </label>

    <label>
      <span>{t("conditional.when")}</span>
      <select bind:value={trigger}>
        {#each TRIGGERS as name (name)}
          <option value={name}>{t(`conditional.trigger.${name}`)}</option>
        {/each}
      </select>
    </label>

    {#if trailing}
      <label>
        <span id="{id}-offset">{t("conditional.offset")}</span>
        <span class="pair">
          <input class="num" type="number" aria-labelledby="{id}-offset" min="0" step="any" bind:value={offset} />
          <select aria-label={t("conditional.unit")} bind:value={percent}>
            <option value={false}>{t("conditional.unit.amount")}</option>
            <option value={true}>{t("conditional.unit.percent")}</option>
          </select>
        </span>
      </label>
    {:else}
      <label>
        <span id="{id}-trigger">{t("conditional.price")}</span>
        <span class="stepper">
          <!-- step="any": tick ladders need not start on a multiple of their step. -->
          <input
            class="num"
            type="number"
            aria-labelledby="{id}-trigger"
            min="0"
            step="any"
            placeholder={last ?? ""}
            bind:value={triggerPrice}
          />
          <button
            type="button"
            aria-label={t("ticket.tickDown")}
            title={t("ticket.tickDown")}
            onclick={() => (triggerPrice = step(stepDown, triggerPrice))}>−</button
          >
          <button
            type="button"
            aria-label={t("ticket.tickUp")}
            title={t("ticket.tickUp")}
            onclick={() => (triggerPrice = step(stepUp, triggerPrice))}>+</button
          >
        </span>
      </label>
    {/if}

    <label>
      <span>
        {t("conditional.sendAs")}
        {#if sendAs === "marketable"}<em class="hint">{t(banded ? "conditional.marketableHint.band" : "conditional.marketableHint.market")}</em>{/if}
      </span>
      <select bind:value={sendAs}>
        <option value="marketable">{t("conditional.sendAs.marketable")}</option>
        <option value="limit">{t("conditional.sendAs.limit")}</option>
      </select>
    </label>

    {#if sendAs === "limit"}
      <label>
        <span id="{id}-limit">{t("ticket.limitPrice")}</span>
        <input class="num" type="number" aria-labelledby="{id}-limit" min="0" step="any" bind:value={limitPrice} />
      </label>
    {/if}

    <button class="submit {side}" type="submit" disabled={!ready || busy}>
      {busy ? t("conditional.placing") : t("conditional.submit", { side: t(`side.${side}`), symbol: selected ?? "" })}
    </button>

    {#if message}
      <p class="msg {message.kind}" role="status">{message.text}</p>
    {/if}
    <p class="lifetime">{t(market.journal?.kept ? "conditional.lifetimeKept" : "conditional.lifetime")}</p>
  </form>

  <section aria-labelledby="{id}-held">
    <h2 id="{id}-held">{t("conditional.held")}</h2>
    {#if cancelError}<p class="msg err" role="alert">{cancelError}</p>{/if}
    {#if market.conditionalList.length}
      <ol class="held">
        {#each market.conditionalList as conditional (conditional.id)}
          <li class:done={!isLive(conditional)}>
            <span class="what">
              <span class={conditional.side === "buy" ? "up" : "down"}>{t(`side.${conditional.side}`)}</span>
              <span class="num">{formatQuantity(conditional.quantity)}</span>
              <span class="sym">{conditional.symbol}</span>
              {#if conditional.after_order != null}
                <span class="dim">{t("conditional.exitOf", { id: conditional.after_order })}</span>
              {/if}
            </span>
            <span class="when">
              {condition(conditional)}
              {#if conditional.trigger === "trailing" && conditional.level && conditional.status === "waiting"}
                <span class="dim">{t("conditional.level", { price: price(conditional.level, market.priceDecimals(conditional.symbol)) })}</span>
              {/if}
            </span>
            <span class="status">
              {t(`conditional.status.${conditional.status}`)}
              {#if conditional.status === "pending"}
                <span class="dim">{t("conditional.pendingNote", { id: conditional.after_order })}</span>
              {/if}
              {#if conditional.status === "fired"}<span class="dim num">{sentOrder(conditional)}</span>{/if}
            </span>
            {#if isLive(conditional)}
              <button
                type="button"
                class="cancel"
                aria-label={t("conditional.cancelOne", { id: conditional.id })}
                onclick={() => cancel(conditional)}>{t("conditional.cancel")}</button
              >
            {/if}
            {#if conditional.status === "waiting" && conditional.blocked}
              <p class="blocked">{t("conditional.blocked", { reason: conditional.blocked })}</p>
            {/if}
            {#if conditional.problem}
              <p class="blocked">{t("conditional.problem", { reason: conditional.problem })}</p>
            {/if}
          </li>
        {/each}
      </ol>
    {:else}
      <p class="dim none">{t("conditional.none")}</p>
    {/if}
  </section>
</div>

<style>
  .conditionals {
    display: grid;
    align-content: start;
    gap: 0.75rem;
    padding: 0.625rem 1rem 0.875rem;
  }

  header {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    gap: 0.5rem;
    min-width: 0;
  }

  .sym {
    font-family: var(--mono);
    font-weight: 600;
  }

  .dim {
    color: var(--text-dim);
  }

  .up {
    color: var(--up);
  }

  .down {
    color: var(--down);
  }

  .sides {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 0.375rem;
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

  /* Two columns of fields while the panel is wide enough; one when compact. */
  form {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 0.625rem;
  }

  .conditionals:global([data-compact]) form {
    grid-template-columns: minmax(0, 1fr);
  }

  label {
    display: grid;
    align-content: start;
    gap: 0.25rem;
    min-width: 0;
    font-size: var(--text-xs);
    color: var(--text-dim);
    text-transform: uppercase;
    letter-spacing: 0.1em;
  }

  .hint {
    margin-left: 0.375rem;
    font-style: normal;
    text-transform: none;
    letter-spacing: 0;
  }

  .conditionals:global([data-narrow]) .hint {
    display: none;
  }

  input,
  select {
    width: 100%;
    min-width: 0;
    padding: 0.5rem 0.625rem;
    background: var(--ink);
    border: 1px solid var(--rule);
    border-radius: 0;
    font-size: var(--text-md);
  }

  input:focus,
  select:focus {
    outline: none;
    border-color: var(--accent);
  }

  .stepper {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto auto;
  }

  .stepper button {
    width: 1.875rem;
    border: 1px solid var(--rule);
    border-left: none;
    background: var(--panel);
    color: var(--text-dim);
    cursor: pointer;
  }

  .stepper button:hover,
  .stepper button:focus-visible {
    color: var(--text);
    outline: none;
  }

  .pair {
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(0, 0.6fr);
  }

  .pair select {
    border-left: none;
  }

  .submit,
  .msg,
  .lifetime {
    grid-column: 1 / -1;
  }

  .submit {
    padding: 0.625rem;
    border: none;
    font: inherit;
    font-weight: 700;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--ink);
    cursor: pointer;
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
    font-size: var(--text-sm);
  }

  .msg.ok {
    color: var(--text-dim);
  }

  .msg.err {
    color: var(--down);
  }

  .lifetime {
    margin: 0;
    font-size: var(--text-xs);
    color: var(--text-dim);
  }

  h2 {
    margin: 0 0 0.375rem;
    font-size: var(--text-xs);
    font-weight: 600;
    color: var(--text-dim);
    text-transform: uppercase;
    letter-spacing: 0.1em;
  }

  .none {
    margin: 0;
    font-size: var(--text-sm);
  }

  .held {
    display: grid;
    gap: 0.25rem;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  /* One line per held order where it fits: what, when, status, cancel. */
  .held li {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr) auto auto;
    align-items: baseline;
    gap: 0.25rem 0.625rem;
    padding: 0.375rem 0.5rem;
    background: var(--panel-raised);
    border-left: 3px solid var(--accent);
    font-size: var(--text-sm);
  }

  .held li.done {
    border-left-color: var(--rule);
    color: var(--text-dim);
  }

  .conditionals:global([data-compact]) .held li {
    grid-template-columns: minmax(0, 1fr) auto;
  }

  .conditionals:global([data-compact]) .cancel {
    grid-column: 2;
    grid-row: 1;
  }

  .conditionals:global([data-compact]) .when,
  .conditionals:global([data-compact]) .status {
    grid-column: 1 / -1;
  }

  .what {
    display: flex;
    flex-wrap: wrap;
    gap: 0 0.375rem;
    min-width: 0;
  }

  .when {
    min-width: 0;
    overflow-wrap: anywhere;
  }

  .status {
    display: flex;
    flex-wrap: wrap;
    gap: 0 0.375rem;
  }

  .cancel {
    padding: 0.125rem 0.5rem;
    border: 1px solid var(--rule);
    background: transparent;
    color: var(--text-dim);
    font: inherit;
    font-size: var(--text-xs);
    cursor: pointer;
  }

  .cancel:hover,
  .cancel:focus-visible {
    color: var(--down);
    border-color: var(--down);
    outline: none;
  }

  .blocked {
    grid-column: 1 / -1;
    margin: 0;
    color: var(--down);
  }
</style>
