<script>
  // The menu bar (ADR 0017), filled from the menu registry, so a menu or an
  // item a plugin registers before the app mounts shows here with no change
  // to this file. It keeps Windows' keys: Alt or F10 moves to the bar, the
  // arrows walk it, Enter or Down opens a menu, Esc closes it and Esc again
  // goes back to where the keyboard was; while one menu is open, pointing
  // at another opens that one instead.
  import { tick } from "svelte";
  import { listMenus } from "./menus.js";
  import { hotkeys, keyOf, runAction } from "./hotkeys.svelte.js";
  import { t } from "../i18n/i18n.svelte.js";

  /** Shows the trader what a command reports. */
  let { onNotice = () => {} } = $props();

  const registered = listMenus();

  /** The open menu's id, or null. */
  let open = $state(null);
  /** The open submenu: `{ key, choices }`, or null. */
  let sub = $state(null);
  /** The bar button the keyboard lands on: one tab stop for the whole bar. */
  let current = $state(0);
  let bar = $state(null);
  let menuEl = $state(null);
  const buttons = [];
  /** What had the keyboard before the bar took it: the Edit commands act on it, and Esc goes back to it. */
  let before = null;
  /** Alt went down with nothing else: if it comes up the same way, the bar takes the keyboard. */
  let altAlone = false;

  const actionTitle = (id) => {
    const action = hotkeys.actions.find((a) => a.id === id);
    return action ? t(action.title) : null;
  };

  // An item naming an action nobody registered (a plugin that did not load)
  // is left out rather than shown dead, and a menu left with nothing is not
  // shown at all.
  const groupsOf = (menu) =>
    menu.groups
      .map((items) => items.filter((item) => item.kind !== "action" || actionTitle(item.action) !== null))
      .filter((items) => items.length > 0);
  const menus = $derived(registered.filter((menu) => groupsOf(menu).length > 0));

  const keyFor = (item) => (item.kind === "action" ? keyOf(item.action) : item.hint) ?? null;
  const indexOf = (id) => menus.findIndex((menu) => menu.id === id);
  const topItems = () => [...(menuEl?.querySelectorAll(":scope > button, :scope > .has-sub > button") ?? [])];
  const focusAt = (list, at) => list[(at + list.length) % list.length]?.focus();

  // Also on pointer-down, which comes before a click moves the focus: the
  // field it leaves is the one Edit acts on.
  function remember() {
    if (!bar?.contains(document.activeElement)) before = document.activeElement;
  }

  function goBack() {
    if (before && document.contains(before)) before.focus();
    before = null;
  }

  async function show(id, focus = "menu") {
    remember();
    open = id;
    sub = null;
    current = indexOf(id);
    await tick();
    if (focus === "first") topItems()[0]?.focus();
    else if (focus === "last") topItems().at(-1)?.focus();
    else menuEl?.focus();
  }

  function close(refocus = true) {
    const at = indexOf(open);
    open = null;
    sub = null;
    if (refocus && at >= 0) buttons[at]?.focus();
  }

  function leave() {
    close(false);
    goBack();
  }

  function move(at, step) {
    const next = (at + step + menus.length) % menus.length;
    current = next;
    if (open) show(menus[next].id, "first");
    else buttons[next]?.focus();
  }

  function report(notice) {
    if (notice) onNotice(notice);
  }

  const failed = (err) => report({ id: "hotkey.failed", vars: { error: String(err) } });

  /** Runs a command or a choice, showing whatever notice it returns or error it throws. */
  function settle(run) {
    try {
      Promise.resolve(run()).then(report, failed);
    } catch (err) {
      failed(err);
    }
  }

  // Runs before any await, so a copy still counts as the trader's own act.
  function pick(item) {
    close(false);
    goBack();
    if (item.kind === "action") runAction(item.action);
    else settle(item.run);
  }

  function choose(choice) {
    close(false);
    goBack();
    settle(choice.pick);
  }

  async function openSub(item, key, focusFirst = false) {
    if (sub?.key !== key) sub = { key, choices: item.choices() };
    if (!focusFirst) return;
    await tick();
    menuEl?.querySelector(".sub > button")?.focus();
  }

  /** The keyboard follows the pointer inside an open menu, so the two never disagree about where it is. */
  function hover(event) {
    event.currentTarget.focus();
  }

  function onBarKeydown(event, at) {
    switch (event.key) {
      case "ArrowRight":
        move(at, 1);
        break;
      case "ArrowLeft":
        move(at, -1);
        break;
      case "Home":
        move(0, 0);
        break;
      case "End":
        move(menus.length - 1, 0);
        break;
      case "ArrowDown":
      case "Enter":
      case " ":
        show(menus[at].id, "first");
        break;
      case "ArrowUp":
        show(menus[at].id, "last");
        break;
      case "Escape":
        leave();
        break;
      default:
        return;
    }
    event.preventDefault();
  }

  function onSubKeydown(event, at) {
    const list = [...event.target.closest(".sub").querySelectorAll("button")];
    const here = list.indexOf(event.target);
    switch (event.key) {
      case "ArrowDown":
        focusAt(list, here + 1);
        break;
      case "ArrowUp":
        focusAt(list, here - 1);
        break;
      case "ArrowLeft":
      case "Escape": {
        const parent = event.target.closest(".has-sub")?.querySelector(":scope > button");
        sub = null;
        parent?.focus();
        break;
      }
      case "ArrowRight":
        move(at, 1);
        break;
      case "Tab":
        leave();
        break;
      default:
        return;
    }
    event.preventDefault();
  }

  function onMenuKeydown(event, at) {
    if (event.target.closest(".sub")) {
      onSubKeydown(event, at);
      return;
    }
    const list = topItems();
    const here = list.indexOf(event.target);
    switch (event.key) {
      case "ArrowDown":
        focusAt(list, here + 1);
        break;
      case "ArrowUp":
        focusAt(list, here < 0 ? list.length - 1 : here - 1);
        break;
      case "Home":
        list[0]?.focus();
        break;
      case "End":
        list.at(-1)?.focus();
        break;
      case "ArrowRight":
        if (event.target.closest(".has-sub")) return;
        move(at, 1);
        break;
      case "ArrowLeft":
        move(at, -1);
        break;
      case "Escape":
        close();
        break;
      case "Tab":
        leave();
        break;
      default:
        return;
    }
    event.preventDefault();
  }

  function onChoicesKeydown(event, item, key) {
    if (event.key !== "ArrowRight" && event.key !== "Enter" && event.key !== " ") return;
    event.preventDefault();
    event.stopPropagation();
    openSub(item, key, true);
  }

  const boundTo = (combo) => Object.values(hotkeys.bindings).includes(combo);
  const somethingModal = () => document.querySelector("dialog[open]") !== null || hotkeys.recording;

  function takeKeyboard() {
    if (!menus.length) return;
    remember();
    current = 0;
    buttons[0]?.focus();
  }

  function onWindowKeydown(event) {
    if (event.key === "Alt") {
      altAlone = !event.ctrlKey && !event.shiftKey && !event.metaKey;
      return;
    }
    altAlone = false;
    const plainF10 = event.key === "F10" && !event.ctrlKey && !event.altKey && !event.shiftKey && !event.metaKey;
    if (plainF10 && !event.defaultPrevented && !boundTo("F10") && !somethingModal()) {
      event.preventDefault();
      takeKeyboard();
    }
  }

  function onWindowKeyup(event) {
    if (event.key !== "Alt" || !altAlone) return;
    altAlone = false;
    if (somethingModal()) return;
    event.preventDefault();
    if (bar?.contains(document.activeElement)) leave();
    else takeKeyboard();
  }

  function onWindowPointerDown(event) {
    altAlone = false;
    if (open && !bar?.contains(event.target)) {
      close(false);
      before = null;
    }
  }
</script>

<svelte:window
  onkeydown={onWindowKeydown}
  onkeyup={onWindowKeyup}
  onpointerdown={onWindowPointerDown}
  onblur={() => (altAlone = false)}
/>

{#if menus.length}
  <div class="menubar" role="menubar" aria-label={t("menu.bar")} bind:this={bar}>
    {#each menus as menu, at (menu.id)}
      <div class="slot" role="none">
        <button
          bind:this={buttons[at]}
          role="menuitem"
          aria-haspopup="menu"
          aria-expanded={open === menu.id}
          tabindex={at === current ? 0 : -1}
          class:open={open === menu.id}
          onpointerdown={remember}
          onclick={() => (open === menu.id ? close() : show(menu.id))}
          onpointerenter={() => open && open !== menu.id && show(menu.id)}
          onkeydown={(event) => onBarKeydown(event, at)}>{t(menu.title)}</button
        >
        {#if open === menu.id}
          <div
            class="menu"
            role="menu"
            aria-label={t(menu.title)}
            tabindex="-1"
            bind:this={menuEl}
            onkeydown={(event) => onMenuKeydown(event, at)}
          >
            {#each groupsOf(menu) as items, g (g)}
              {#if g > 0}<div class="rule" role="separator"></div>{/if}
              {#each items as item, k (k)}
                {#if item.kind === "choices"}
                  <div class="has-sub" role="none">
                    <button
                      role="menuitem"
                      aria-haspopup="menu"
                      aria-expanded={sub?.key === `${g}-${k}`}
                      onclick={() => openSub(item, `${g}-${k}`)}
                      onpointerenter={(event) => {
                        hover(event);
                        openSub(item, `${g}-${k}`);
                      }}
                      onkeydown={(event) => onChoicesKeydown(event, item, `${g}-${k}`)}
                    >
                      <span>{t(item.title)}</span><span class="key" aria-hidden="true">›</span>
                    </button>
                    {#if sub?.key === `${g}-${k}`}
                      <div class="menu sub" role="menu" aria-label={t(item.title)}>
                        {#each sub.choices as choice (choice.id)}
                          <button
                            role={choice.checked === undefined ? "menuitem" : "menuitemradio"}
                            aria-checked={choice.checked}
                            onclick={() => choose(choice)}
                            onpointerenter={hover}
                          >
                            <span class="check" aria-hidden="true">{choice.checked ? "●" : ""}</span>
                            <span>{choice.label}</span>
                          </button>
                        {/each}
                      </div>
                    {/if}
                  </div>
                {:else}
                  <button
                    role="menuitem"
                    aria-keyshortcuts={keyFor(item) ?? undefined}
                    onclick={() => pick(item)}
                    onpointerenter={(event) => {
                      sub = null;
                      hover(event);
                    }}
                  >
                    <span>{item.kind === "action" ? actionTitle(item.action) : t(item.title)}</span>
                    <span class="key" aria-hidden="true">{keyFor(item) ?? ""}</span>
                  </button>
                {/if}
              {/each}
            {/each}
          </div>
        {/if}
      </div>
    {/each}
  </div>
{/if}

<style>
  .menubar {
    display: flex;
    align-items: stretch;
    min-width: 0;
  }

  .slot {
    position: relative;
    display: flex;
  }

  .slot > button {
    padding: 0.25rem 0.625rem;
    font-size: var(--text-md);
    color: var(--text);
    background: transparent;
    border: none;
    border-radius: 0.25rem;
    cursor: pointer;
    white-space: nowrap;
  }

  .slot > button:hover,
  .slot > button.open,
  .slot > button:focus-visible {
    background: color-mix(in srgb, var(--text) 10%, transparent);
    outline: none;
  }

  .menu {
    position: absolute;
    top: 100%;
    left: 0;
    z-index: 1000;
    display: flex;
    flex-direction: column;
    min-width: 16rem;
    padding: 0.25rem;
    background: var(--panel-raised);
    border: 1px solid var(--rule);
    border-radius: 0.375rem;
    box-shadow: var(--shadow-menu);
  }

  .menu:focus {
    outline: none;
  }

  .menu button {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 2rem;
    width: 100%;
    padding: 0.375rem 0.625rem;
    font-size: var(--text-md);
    text-align: left;
    white-space: nowrap;
    color: var(--text);
    background: transparent;
    border: none;
    border-radius: 0.25rem;
    cursor: pointer;
  }

  .menu button:hover,
  .menu button:focus-visible,
  .menu button:focus,
  .has-sub > button[aria-expanded="true"] {
    background: color-mix(in srgb, var(--accent) 16%, transparent);
    outline: none;
  }

  .key {
    font-family: var(--mono);
    font-size: var(--text-xs);
    color: var(--text-dim);
  }

  .rule {
    margin: 0.25rem 0.375rem;
    border-top: 1px solid var(--rule);
  }

  .has-sub {
    position: relative;
  }

  .sub {
    top: -0.3125rem;
    left: 100%;
  }

  .sub button {
    justify-content: flex-start;
    gap: 0.5rem;
  }

  .check {
    width: 1rem;
    color: var(--accent);
  }
</style>
