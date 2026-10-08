<script>
  import { untrack } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { market } from "./market.svelte.js";
  import { price, quantity as formatQuantity } from "./format.js";
  import { stepDown, stepUp, tickTable } from "./ticks.js";
  import { amendProblem, amendTerm, canAmendOrder } from "./amend.js";
  import { panelSize } from "./panelSize.svelte.js";
  import { t } from "../i18n/i18n.svelte.js";

  // `order` is the live order: it changes as fills arrive, so the form
  // keeps what was typed and only starts from the order's terms.
  let { order, onclose } = $props();

  const id = $props.id();
  const instrument = $derived(market.instruments[order.symbol]);
  const lot = $derived(Number(instrument?.lot_size) || 1);
  const ticks = $derived(tickTable(instrument));
  const decimals = $derived(market.priceDecimals(order.symbol));
  const band = $derived(market.boards[order.symbol]);

  let quantity = $state(untrack(() => Number(order.quantity)));
  let limitPrice = $state(untrack(() => Number(order.limit_price)));
  let busy = $state(false);
  let error = $state(null);

  const live = $derived(canAmendOrder(order, market.canAmend));
  const problem = $derived(amendProblem(order, quantity, limitPrice));

  // One line under the fields says why Send is off, or what the venue
  // said. While a send is in flight, the order leaving the amendable
  // states is this form's own doing, not news.
  const note = $derived.by(() => {
    if (error) return { text: error, alert: true };
    if (!live && !busy) {
      return { text: t("amend.gone", { id: order.id, status: t(`status.${order.status}`) }), alert: true };
    }
    if (problem === "belowFilled") {
      return { text: t("amend.belowFilled", { filled: formatQuantity(order.filled_quantity) }), alert: true };
    }
    if (problem === "unchanged") return { text: t("amend.unchanged"), alert: false };
    if (problem) return { text: t("amend.missing"), alert: false };
    return null;
  });

  function stepLimit(step) {
    error = null;
    limitPrice = step(ticks, Number(limitPrice) || Number(order.limit_price) || 0);
  }

  function focus(node) {
    node.focus();
    node.select();
  }

  let form;
  function onKeydown(event) {
    if (event.key === "Escape" && form?.contains(event.target)) onclose();
  }

  async function send(event) {
    event.preventDefault();
    if (!live || problem !== null || busy) return;
    busy = true;
    error = null;
    try {
      await invoke("amend_order", {
        id: order.id,
        quantity: amendTerm(quantity, order.quantity),
        limitPrice: amendTerm(limitPrice, order.limit_price),
      });
      onclose();
    } catch (err) {
      error = String(err);
    } finally {
      busy = false;
    }
  }
</script>

<svelte:window onkeydown={onKeydown} />

<!-- novalidate: as on the ticket, risk judges lots and ticks and says why.
     A refusal stands only until the terms it refused are edited. -->
<form
  class="amend"
  use:panelSize
  bind:this={form}
  onsubmit={send}
  oninput={() => (error = null)}
  novalidate
  aria-label={t("amend.title", { id: order.id })}
>
  <p class="what">
    <strong>{t("amend.title", { id: order.id })}</strong>
    <span class={order.side === "buy" ? "up" : "down"}>{t(`side.${order.side}`)}</span>
    <span class="sym">{order.symbol}</span>
    <span class="dim">
      {t("amend.filled", { filled: formatQuantity(order.filled_quantity), quantity: formatQuantity(order.quantity) })}
    </span>
  </p>

  <div class="field">
    <label for="{id}-quantity">
      {t("amend.quantity")}
      {#if lot > 1}<em class="hint">{t("ticket.lot", { lot: formatQuantity(lot) })}</em>{/if}
    </label>
    <input id="{id}-quantity" class="num" type="number" min={lot} step={lot} bind:value={quantity} />
  </div>

  <div class="field">
    <label for="{id}-price">
      {t("amend.limitPrice")}
      {#if band?.ceiling && band?.floor}
        <em class="hint">
          {t("ticket.band", { floor: price(band.floor, decimals), ceiling: price(band.ceiling, decimals) })}
        </em>
      {/if}
    </label>
    <span class="stepper">
      <input id="{id}-price" class="num" type="number" min="0" step="any" bind:value={limitPrice} use:focus />
      <button type="button" aria-label={t("ticket.tickDown")} title={t("ticket.tickDown")} onclick={() => stepLimit(stepDown)}
        >−</button
      >
      <button type="button" aria-label={t("ticket.tickUp")} title={t("ticket.tickUp")} onclick={() => stepLimit(stepUp)}
        >+</button
      >
    </span>
  </div>

  <span class="buttons">
    <button
      class="send"
      type="submit"
      disabled={!live || problem !== null || busy}
      aria-describedby={note ? `${id}-note` : undefined}
    >
      {busy ? t("amend.sending") : t("amend.send")}
    </button>
    <button class="close" type="button" onclick={onclose}>{t("amend.close")}</button>
  </span>

  {#if note}
    <p id="{id}-note" class="note" class:alert={note.alert} role={error ? "alert" : "status"}>{note.text}</p>
  {/if}
</form>

<style>
  .amend {
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(0, 1fr) auto;
    align-items: end;
    gap: 0.5rem 0.625rem;
    margin: 0 1rem 0.5rem;
    padding: 0.5rem 0.625rem 0.625rem;
    background: var(--panel-raised);
    border-left: 3px solid var(--accent);
  }

  .what {
    grid-column: 1 / -1;
    display: flex;
    flex-wrap: wrap;
    gap: 0.25rem 0.625rem;
    align-items: baseline;
    margin: 0;
    font-size: var(--text-sm);
  }

  .sym {
    font-family: var(--mono);
    font-weight: 600;
  }

  .up {
    color: var(--up);
  }

  .down {
    color: var(--down);
  }

  .field {
    display: grid;
    gap: 0.25rem;
  }

  label {
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

  input {
    width: 100%;
    min-width: 0;
    padding: 0.375rem 0.5rem;
    background: var(--ink);
    border: 1px solid var(--rule);
    border-radius: 0;
    font-size: var(--text-md);
  }

  input:focus {
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

  .buttons {
    display: flex;
    gap: 0.375rem;
  }

  /* Compact: the fields stack, and the buttons take a row of their own. */
  .amend:global([data-compact]) {
    grid-template-columns: minmax(0, 1fr);
  }

  .amend:global([data-compact]) .buttons > * {
    flex: 1;
  }

  .amend:global([data-narrow]) .hint {
    display: none;
  }

  .send,
  .close {
    padding: 0.4375rem 0.75rem;
    font: inherit;
    font-size: var(--text-xs);
    font-weight: 700;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    cursor: pointer;
  }

  .send {
    border: none;
    background: var(--accent);
    color: var(--ink);
  }

  .send:disabled {
    opacity: 0.5;
    cursor: default;
  }

  .close {
    border: 1px solid var(--rule);
    background: transparent;
    color: var(--text-dim);
  }

  .close:hover,
  .close:focus-visible {
    color: var(--text);
    outline: none;
  }

  .note {
    grid-column: 1 / -1;
    margin: 0;
    font-size: var(--text-sm);
    color: var(--text-dim);
  }

  .note.alert {
    color: var(--down);
  }
</style>
