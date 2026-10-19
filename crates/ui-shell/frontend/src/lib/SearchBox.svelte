<script>
  // Search in the title bar (ADR 0017): commands, a screen to add, a theme,
  // a symbol to show, found by typing. Ctrl+K (the "app.search" action,
  // rebindable) puts the keyboard here; the arrows walk the results, Enter
  // runs one, and Esc goes back to where the keyboard was.
  import { onMount, tick } from "svelte";
  import { hotkeys, keyOf, runAction } from "./hotkeys.svelte.js";
  import { listMenus } from "./menus.js";
  import { market } from "./market.svelte.js";
  import { setSharedSymbol } from "./links.svelte.js";
  import { SEARCH_EVENT } from "./searchEvent.js";
  import { commandEntries, search, symbolEntries } from "./search.js";
  import { t } from "../i18n/i18n.svelte.js";

  /**
   * Shows the trader what a command reports; `onPick` runs when a symbol is
   * picked, so the frame can show the panels that follow it.
   */
  let { onNotice = () => {}, onPick = () => {} } = $props();

  let query = $state("");
  let open = $state(false);
  let current = $state(0);
  let input = $state(null);
  let before = null;

  // Built when the trader types, so a menu's choices (which theme is
  // checked, which screens exist) are read as they are now.
  const results = $derived.by(() => {
    if (!open || !query.trim()) return [];
    const commands = commandEntries({
      actions: hotkeys.actions.filter((action) => action.id !== "app.search"),
      menus: listMenus(),
      translate: t,
      keyOf,
      runAction,
    });
    const symbols = symbolEntries(market.listed, market.instruments, (symbol) => {
      setSharedSymbol(symbol);
      onPick(symbol);
    });
    return search(query, [...symbols, ...commands]);
  });

  $effect(() => {
    if (current >= results.length) current = 0;
  });

  function focusSearch() {
    if (document.activeElement !== input) before = document.activeElement;
    open = true;
    input?.focus();
    input?.select();
  }

  /** Whether `element` can take the keyboard back: still in the page, and shown. */
  const showing = (element) => element && document.contains(element) && element.getClientRects().length > 0;

  function close({ back = true } = {}) {
    open = false;
    query = "";
    current = 0;
    if (back && showing(before)) before.focus();
    else input?.blur();
    before = null;
  }

  async function choose(entry) {
    const back = before;
    close({ back: false });
    await tick();
    try {
      const notice = await entry.run();
      if (notice) onNotice(notice);
    } catch (err) {
      onNotice({ id: "hotkey.failed", vars: { error: String(err) } });
    }
    // Back to where the keyboard was, unless the command moved it or hid it.
    if (document.activeElement === document.body && showing(back)) back.focus();
  }

  function onKeydown(event) {
    // A key that finishes an input method's word is not a command.
    if (event.isComposing || event.keyCode === 229) return;
    if (event.key === "Escape") {
      event.preventDefault();
      close();
    } else if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      if (!results.length) return;
      const step = event.key === "ArrowDown" ? 1 : -1;
      current = (current + step + results.length) % results.length;
    } else if (event.key === "Enter") {
      event.preventDefault();
      if (results[current]) choose(results[current]);
    }
  }

  onMount(() => {
    window.addEventListener(SEARCH_EVENT, focusSearch);
    return () => window.removeEventListener(SEARCH_EVENT, focusSearch);
  });

  const hint = $derived(keyOf("app.search"));
  const optionId = (index) => `search-result-${index}`;
</script>

<div class="search" data-popup={open && results.length ? "" : undefined}>
  <input
    bind:this={input}
    bind:value={query}
    type="search"
    role="combobox"
    aria-expanded={open && query.trim() !== ""}
    aria-controls={open && query.trim() !== "" ? "search-results" : undefined}
    aria-activedescendant={results.length ? optionId(current) : undefined}
    aria-label={t("search.label")}
    placeholder={hint ? t("search.placeholderKey", { key: hint }) : t("search.placeholder")}
    spellcheck="false"
    autocomplete="off"
    onfocus={() => (open = true)}
    oninput={() => {
      open = true;
      current = 0;
    }}
    onkeydown={onKeydown}
    onblur={() => setTimeout(() => {
      if (document.activeElement !== input) {
        open = false;
        query = "";
      }
    })}
  />
  {#if open && query.trim()}
    <!-- A combobox's options take the pointer; the keyboard stays in the
         field above, which walks them. -->
    <ul id="search-results" role="listbox" aria-label={t("search.results")}>
      {#each results as entry, index (index)}
        <li
          id={optionId(index)}
          role="option"
          aria-selected={index === current}
          class:current={index === current}
          onpointerdown={(event) => event.preventDefault()}
          onpointermove={() => (current = index)}
          onpointerup={() => choose(entry)}
        >
          <span class="kind">{t(`search.kind.${entry.kind}`)}</span>
          <span class="label">{entry.label}</span>
          {#if entry.detail}<span class="detail">{entry.detail}</span>{/if}
          {#if entry.key}<kbd>{entry.key}</kbd>{/if}
        </li>
      {/each}
    </ul>
    {#if !results.length}<p class="none" role="status">{t("search.none")}</p>{/if}
  {/if}
</div>

<style>
  .search {
    position: relative;
    align-self: center;
    flex: 0 1 22rem;
    min-width: 5rem;
  }

  input {
    width: 100%;
    padding: 0.25rem 0.625rem;
    font-size: var(--text-sm);
    color: var(--text);
    background: var(--panel);
    border: 1px solid var(--rule);
    border-radius: 0.25rem;
  }

  input::placeholder {
    color: var(--text-dim);
  }

  input:focus {
    border-color: var(--accent);
    outline: none;
  }

  ul,
  .none {
    position: absolute;
    top: calc(100% + 0.25rem);
    right: 0;
    z-index: 50;
    width: max(100%, min(28rem, 90vw));
    margin: 0;
    background: var(--panel-raised);
    border: 1px solid var(--rule);
    box-shadow: var(--shadow-menu);
  }

  ul {
    max-height: min(26rem, 70vh);
    padding: 0.25rem 0;
    overflow-y: auto;
    list-style: none;
  }

  li {
    display: flex;
    align-items: baseline;
    gap: 0.625rem;
    min-width: 0;
    padding: 0.375rem 0.75rem;
    font-size: var(--text-md);
    cursor: pointer;
  }

  li.current {
    background: color-mix(in srgb, var(--accent) 16%, transparent);
  }

  .kind {
    flex: none;
    width: 4.5rem;
    font-size: var(--text-xs);
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: var(--text-dim);
  }

  .label {
    flex: none;
    max-width: 60%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .detail {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--text-sm);
    color: var(--text-dim);
  }

  kbd {
    margin-left: auto;
    flex: none;
    font-family: var(--mono);
    font-size: var(--text-xs);
    color: var(--text-dim);
  }

  .none {
    padding: 0.5rem 0.75rem;
    font-size: var(--text-md);
    color: var(--text-dim);
  }
</style>
