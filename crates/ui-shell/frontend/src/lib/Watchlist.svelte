<script>
  // A watchlist panel shows one list: "All" (every symbol the venue lists)
  // or one of the trader's named lists, which it can rename, delete, and
  // fill. The lists are shared by every panel in every window; which one a
  // panel shows is its own `list` parameter, saved with the layout.
  import { Grid } from "bo-grid";
  import { keyboardRow } from "./keyboardRow.js";
  import { ROW_HEIGHT_PX, gridTheme } from "./gridTheme.js";
  import { px } from "./ui.svelte.js";
  import { market } from "./market.svelte.js";
  import { chooseSymbol, symbolOf } from "./links.svelte.js";
  import { setLists, startWatchlists, watchlists } from "./watchlists.svelte.js";
  import {
    ALL,
    MAX_NAME,
    addProblem,
    cleanSymbol,
    moved,
    nextListName,
    renamed,
    symbolsOf,
    withList,
    withSymbol,
    withoutList,
    withoutSymbol,
  } from "./watchlists.js";
  import RemoveCell from "./RemoveCell.svelte";
  import { panelSize } from "./panelSize.svelte.js";
  import { t, tn } from "../i18n/i18n.svelte.js";
  import { price } from "./format.js";

  let { params, panel } = $props();

  const id = $props.id();

  $effect(() => {
    startWatchlists().catch((err) => (watchlists.error = String(err)));
  });

  const list = $derived(watchlists.lists.find((l) => l.id === params.list) ?? null);
  const listId = $derived(list?.id ?? ALL);
  const symbols = $derived(symbolsOf(listId, watchlists.lists, market.symbols));
  const selected = $derived(symbolOf(params, symbols[0] ?? market.symbols[0], market.listed));

  /** Every symbol the venue lists, with its name: what a list can hold. */
  const listed = $derived(
    new Map([
      ...market.symbols.map((symbol) => [symbol, ""]),
      ...Object.values(market.instruments).map((info) => [info.symbol, info.name ?? ""]),
    ]),
  );
  const suggestions = $derived(list ? [...listed].filter(([symbol]) => !list.symbols.includes(symbol)) : []);

  /** Linked: every linked panel follows. Not linked: only this panel remembers it. */
  const choose = (symbol) => chooseSymbol(params, panel, symbol);

  function showList(next) {
    disarmDelete();
    renaming = false;
    note = null;
    typed = "";
    panel?.updateParameters({ list: next });
  }

  // Renaming swaps the list picker for a field; Enter or leaving the field
  // keeps the name, Escape drops it.
  let renaming = $state(false);
  let draftName = $state("");

  function startRename(name) {
    draftName = name;
    renaming = true;
  }

  function finishRename(keep) {
    if (!renaming) return;
    renaming = false;
    if (keep && list) setLists(renamed(watchlists.lists, list.id, draftName));
  }

  function focusSelect(node) {
    node.focus();
    node.select();
  }

  function newList() {
    const name = nextListName(watchlists.lists, (n) => t("watchlist.newName", { n }));
    const listIdNew = crypto.randomUUID();
    setLists(withList(watchlists.lists, listIdNew, name));
    showList(listIdNew);
    startRename(name);
  }

  // Deleting takes a second click on the same button, which says what it
  // will do. Leaving the button, or waiting, calls it off: not every
  // webview focuses a clicked button, so blur alone cannot be relied on.
  const CONFIRM_MS = 4000;
  let confirmingDelete = $state(false);
  let disarmTimer = null;

  function disarmDelete() {
    clearTimeout(disarmTimer);
    confirmingDelete = false;
  }

  function deleteList() {
    if (!list) return;
    if (!confirmingDelete) {
      confirmingDelete = true;
      disarmTimer = setTimeout(disarmDelete, CONFIRM_MS);
      return;
    }
    disarmDelete();
    setLists(withoutList(watchlists.lists, list.id));
    showList(ALL);
  }

  $effect(() => () => clearTimeout(disarmTimer));

  let typed = $state("");
  /** `{ text, alert }` under the header: why a symbol was not added, or a save that failed. */
  let note = $state(null);

  function addTyped(event) {
    event.preventDefault();
    if (!list) return;
    const symbol = cleanSymbol(typed);
    const problem = addProblem(list, symbol, listed);
    if (problem === "empty") return;
    if (problem === "notListed") note = { text: t("watchlist.notListed", { symbol }), alert: true };
    else if (problem === "already") note = { text: t("watchlist.already", { symbol, name: list.name }), alert: false };
    else {
      setLists(withSymbol(watchlists.lists, list.id, symbol));
      note = null;
      typed = "";
    }
  }

  function remove(symbol) {
    if (list) setLists(withoutSymbol(watchlists.lists, list.id, symbol));
  }

  function reorder(from, to) {
    if (list) setLists(moved(watchlists.lists, list.id, from, to));
  }

  /** At the precision of the instrument's tick: no decimals for VND, cents for USD. */
  const atTick = (value, row) => price(value, row && market.priceDecimals(row.symbol));

  // A named list keeps the trader's order, which dragging sets, so its
  // columns do not sort; "All" sorts freely.
  const columns = $derived([
    { type: "text", key: "symbol", header: t("column.symbol"), width: px(70), sortable: !list },
    { type: "sparkline", key: "trend", sparkKey: "trend", header: t("column.trend"), width: px(96), sortable: false },
    { type: "price", key: "bid", header: t("column.bid"), width: px(84), flash: "auto", format: atTick, sortable: !list },
    { type: "price", key: "ask", header: t("column.ask"), width: px(84), flash: "auto", format: atTick, sortable: !list },
    { type: "number", key: "spread", header: t("column.spread"), width: px(64), format: atTick, sortable: !list },
    ...(list
      ? [{ type: "custom", key: "remove", header: "", width: px(36), sortable: false, component: RemoveCell, onRemove: remove }]
      : []),
  ]);

  const num = (value) => (value === undefined || value === null ? null : Number(value));

  const rows = $derived(
    symbols.map((symbol) => {
      const quote = market.quotes[symbol];
      const bid = num(quote?.bid);
      const ask = num(quote?.ask);
      return {
        id: symbol,
        symbol,
        bid,
        ask,
        spread: bid === null || ask === null ? null : ask - bid,
        trend: market.trends[symbol] ?? [],
      };
    }),
  );

  const shownNote = $derived(
    watchlists.error ? { text: t("watchlist.saveFailed", { error: watchlists.error }), alert: true } : note,
  );
</script>

<div class="watch" use:panelSize>
  <header>
    <div class="lists">
      {#if renaming && list}
        <input
          class="name"
          bind:value={draftName}
          maxlength={MAX_NAME}
          aria-label={t("watchlist.name")}
          use:focusSelect
          onkeydown={(event) => {
            if (event.key === "Enter") finishRename(true);
            if (event.key === "Escape") finishRename(false);
          }}
          onblur={() => finishRename(true)}
        />
      {:else}
        <select value={listId} aria-label={t("watchlist.list")} onchange={(event) => showList(event.currentTarget.value)}>
          <option value={ALL}>{t("watchlist.all")}</option>
          {#each watchlists.lists as each (each.id)}
            <option value={each.id}>{each.name}</option>
          {/each}
        </select>
      {/if}
      <!-- Editing waits for the saved lists: a change on top of lists that
           never loaded would save over them. -->
      {#if watchlists.ready}
        <button onclick={newList}>{t("watchlist.new")}</button>
        {#if list && !renaming}
          <button onclick={() => startRename(list.name)}>{t("watchlist.rename")}</button>
          <button class:danger={confirmingDelete} onclick={deleteList} onblur={disarmDelete} onmouseleave={disarmDelete}>
            {confirmingDelete ? t("watchlist.confirmDelete", { name: list.name }) : t("watchlist.delete")}
          </button>
        {/if}
      {/if}
    </div>
    <span class="dim count">{tn("watchlist.count", symbols.length)}</span>
  </header>

  {#if list && watchlists.ready}
    <form class="add" onsubmit={addTyped}>
      <input
        list="{id}-symbols"
        bind:value={typed}
        placeholder={t("watchlist.addPlaceholder")}
        aria-label={t("watchlist.add")}
        oninput={() => (note = null)}
      />
      <datalist id="{id}-symbols">
        {#each suggestions as [symbol, name] (symbol)}
          <option value={symbol}>{name}</option>
        {/each}
      </datalist>
      <button type="submit">{t("watchlist.addButton")}</button>
    </form>
  {/if}

  {#if shownNote}
    <p class="note" class:alert={shownNote.alert} role={shownNote.alert ? "alert" : "status"}>{shownNote.text}</p>
  {/if}

  <!-- A fresh grid per list: a sort made in "All" must not carry into a
       named list, whose order is the trader's and whose drag-to-reorder
       works only unsorted. -->
  <div class="grid" use:keyboardRow>
    {#key listId}
      <Grid
        {rows}
        {columns}
        height="100%"
        theme={gridTheme}
        rowHeight={px(ROW_HEIGHT_PX)}
        fitColumns
        cellSelection={false}
        selectedRowId={selected}
        onRowClick={(row) => choose(row.symbol)}
        onRowReorder={list ? reorder : undefined}
        emptyMessage={list ? t("watchlist.empty") : t("feed.waiting")}
      />
    {/key}
  </div>

  <p class="hint dim">{t("watchlist.hint")}</p>
</div>

<style>
  .watch {
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    grid-template-rows: auto auto auto minmax(0, 1fr) auto;
    height: 100%;
    min-height: 0;
  }

  header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 0.625rem;
    padding: 0.5rem 1rem 0.375rem;
  }

  .lists {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.375rem;
    min-width: 0;
  }

  .count {
    flex: none;
  }

  select,
  input {
    min-width: 0;
    padding: 0.25rem 0.5rem;
    font: inherit;
    font-size: var(--text-sm);
    background: var(--ink);
    border: 1px solid var(--rule);
    border-radius: 0;
    color: var(--text);
  }

  select {
    max-width: 11.25rem;
  }

  .name {
    width: 10rem;
  }

  select:focus,
  input:focus {
    outline: none;
    border-color: var(--accent);
  }

  button {
    padding: 0.1875rem 0.5625rem;
    font: inherit;
    font-size: var(--text-xs);
    text-transform: uppercase;
    letter-spacing: 0.06em;
    background: transparent;
    border: 1px solid var(--rule);
    color: var(--text-dim);
    cursor: pointer;
  }

  button:hover,
  button:focus-visible {
    color: var(--text);
    border-color: var(--accent);
    outline: none;
  }

  button.danger {
    color: var(--down);
    border-color: var(--down);
  }

  .add {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    gap: 0.375rem;
    padding: 0 1rem 0.5rem;
  }

  .note {
    margin: 0 1rem 0.5rem;
    font-size: var(--text-sm);
    color: var(--text-dim);
  }

  .note.alert {
    color: var(--down);
  }

  /* Second to last whichever of the add field and the note are showing. */
  .grid {
    grid-row: 4;
    min-height: 0;
  }

  .hint {
    grid-row: 5;
    margin: 0;
    padding: 0.5rem 1rem 0.75rem;
    font-size: var(--text-xs);
  }

  .watch:global([data-compact]) .hint,
  .watch:global([data-narrow]) .count {
    display: none;
  }
</style>
