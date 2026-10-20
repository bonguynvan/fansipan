<script>
  import { onDestroy } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { market } from "./market.svelte.js";
  import { money, price, quantity as formatQuantity } from "./format.js";
  import { debouncedSetter, getSetting } from "./settings.js";
  import { RISK_SETTING, keptBudget, sizingArgs, sizingMessage } from "./sizing.js";
  import { t } from "../i18n/i18n.svelte.js";

  // The ticket's symbol, side, and type, and the price it would enter at:
  // its limit, or its reference for an order without one.
  // `entryTyped` says the entry is the trader's own limit, not the
  // reference moving with the quotes. The stop-loss and take-profit are the
  // ticket's: it sends them with the order as a bracket, and the
  // stop is also what the size is worked out from.
  let {
    symbol,
    side,
    orderType,
    entry,
    entryTyped = false,
    decimals,
    onuse,
    stop = $bindable(null),
    takeProfit = $bindable(null),
  } = $props();
  const id = $props.id();

  let open = $state(false);
  let risk = $state("");
  let percent = $state(false);

  // The budget is the trader's, kept for every ticket and the next session.
  const keep = debouncedSetter(RISK_SETTING, 500);
  onDestroy(() => keep.flush());
  let loaded = false;
  getSetting(RISK_SETTING)
    .then((stored) => {
      // What the trader typed while it loaded stands.
      if (risk !== "") return;
      const kept = keptBudget(stored);
      risk = kept.risk;
      percent = kept.percent;
    })
    .catch(() => {})
    .finally(() => (loaded = true));
  $effect(() => {
    const budget = { risk, percent };
    if (loaded) keep(budget);
  });

  // Core's answer for what is on the ticket now, asked at most every
  // quarter second, as the reference moves with every quote. An older,
  // slower answer never overwrites a newer one. An answer to other terms
  // (a typed limit among them) is cleared at once, so Use never fills a
  // size meant for them; the reference moving alone keeps the last answer
  // in view until the next one arrives.
  const SETTLE_MS = 250;
  let sizing = $state(null);
  let problem = $state(null);
  let asked = 0;
  let askedFor = "";
  $effect(() => {
    const args = open ? sizingArgs({ symbol, side, orderType, entry, stop, risk, percent }) : null;
    const ask = ++asked;
    const terms = args ? JSON.stringify({ ...args, entry: entryTyped ? args.entry : null }) : "";
    if (terms !== askedFor) {
      sizing = null;
      problem = null;
    }
    askedFor = terms;
    if (!args) return;
    const timer = setTimeout(() => {
      invoke("size_for_risk", args)
        .then((answer) => {
          if (ask !== asked) return;
          sizing = answer;
          problem = null;
        })
        .catch((err) => {
          if (ask !== asked) return;
          sizing = null;
          problem = String(err);
        });
    }, SETTLE_MS);
    return () => clearTimeout(timer);
  });

  // These fields sit inside the order form: Enter in one never sends it.
  const staysHere = (event) => event.key === "Enter" && event.preventDefault();

  const currency = $derived(market.info?.currency);
  const words = $derived.by(() => {
    if (!sizing) return null;
    return t(sizingMessage(sizing), {
      quantity: formatQuantity(sizing.quantity),
      // Costs are fractions of a tick: two more places than prices show.
      loss: price(sizing.loss_per_unit, decimals + 2),
      atRisk: money(sizing.at_risk, currency),
      budget: money(sizing.budget, currency),
      bound: sizing.capped_by ? t(`ticket.risk.bound.${sizing.capped_by}`) : "",
    });
  });
</script>

<details class="sizer" bind:open>
  <summary>
    {t("ticket.exits.title")}
    {#if !open && (Number(stop) > 0 || Number(takeProfit) > 0)}
      <span class="set num">
        {[
          Number(stop) > 0 ? t("ticket.exits.stopAt", { price: price(stop, decimals) }) : null,
          Number(takeProfit) > 0 ? t("ticket.exits.targetAt", { price: price(takeProfit, decimals) }) : null,
        ]
          .filter(Boolean)
          .join(" · ")}
      </span>
    {/if}
  </summary>
  <div class="fields">
    <label>
      <span id="{id}-stop">
        {t("ticket.risk.stop")}
        {#if Number(entry) > 0}<em class="hint">{t("ticket.risk.entry", { price: price(entry, decimals) })}</em>{/if}
      </span>
      <input class="num" type="number" aria-labelledby="{id}-stop" min="0" step="any" bind:value={stop} onkeydown={staysHere} />
    </label>
    <label>
      <span id="{id}-target">{t("ticket.exits.takeProfit")}</span>
      <input
        class="num"
        type="number"
        aria-labelledby="{id}-target"
        min="0"
        step="any"
        bind:value={takeProfit}
        onkeydown={staysHere}
      />
    </label>
    <label>
      <span id="{id}-risk">{t("ticket.risk.amount")}</span>
      <span class="pair">
        <input class="num" type="number" aria-labelledby="{id}-risk" min="0" step="any" bind:value={risk} onkeydown={staysHere} />
        <select aria-label={t("ticket.risk.unit")} bind:value={percent}>
          <option value={false}>{currency ?? t("ticket.risk.unit.amount")}</option>
          <option value={true}>{t("ticket.risk.unit.percent")}</option>
        </select>
      </span>
    </label>
  </div>
  {#if problem}
    <p class="note alert" role="status">{problem}</p>
  {:else if words}
    <p class="note" role="status">
      <span>{words}</span>
      {#if Number(sizing.quantity) > 0}
        <button type="button" class="use" onclick={() => onuse(Number(sizing.quantity))}
          >{t("ticket.risk.use", { quantity: formatQuantity(sizing.quantity) })}</button
        >
      {/if}
    </p>
  {/if}
</details>

<style>
  .sizer {
    border: 1px solid var(--rule);
    background: var(--panel-raised);
  }

  summary {
    padding: 0.375rem 0.625rem;
    font-size: var(--text-xs);
    color: var(--text-dim);
    text-transform: uppercase;
    letter-spacing: 0.1em;
    cursor: pointer;
  }

  .set {
    margin-left: 0.5rem;
    color: var(--text);
    text-transform: none;
    letter-spacing: 0;
  }

  summary:hover,
  summary:focus-visible {
    color: var(--text);
    outline: none;
  }

  .fields {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(min(100%, 8rem), 1fr));
    gap: 0.5rem;
    padding: 0 0.625rem 0.5rem;
  }

  label {
    display: grid;
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

  input,
  select {
    width: 100%;
    min-width: 0;
    padding: 0.375rem 0.5rem;
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

  .pair {
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
  }

  .pair select {
    border-left: none;
  }

  .note {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    justify-content: space-between;
    gap: 0.25rem 0.5rem;
    margin: 0;
    padding: 0 0.625rem 0.5rem;
    font-size: var(--text-sm);
    color: var(--text-dim);
  }

  .note span {
    min-width: 0;
  }

  .note.alert {
    color: var(--down);
  }

  .use {
    padding: 0.125rem 0.5rem;
    border: 1px solid var(--accent);
    background: transparent;
    color: var(--accent);
    font: inherit;
    font-size: var(--text-xs);
    font-weight: 600;
    cursor: pointer;
  }

  .use:hover,
  .use:focus-visible {
    background: var(--accent);
    color: var(--ink);
    outline: none;
  }
</style>
