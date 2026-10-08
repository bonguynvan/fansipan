<script>
  // The main window's desktops and pages (ADR 0013): a desktop menu, the
  // open pages as tabs, and "+" for a new page (blank, from a registered
  // layout, or a copy of this one) or a closed page to open again.
  // Double-click a tab to rename it. Removing a desktop or deleting a closed
  // page asks once more on the same button. Tabs and menus are plain buttons
  // in lists: the current page is marked, and each popup list is the
  // disclosure of the button that opens it.
  import { listLayouts } from "./registry.js";
  import * as pages from "./pages.js";
  import {
    closePageTab,
    copyPage,
    deletePage,
    goToDesktop,
    goToPage,
    newDesktop,
    newPage,
    removeDesktop,
    renameDesktop,
    renamePage,
    reopenPage,
    workspace,
  } from "./workspace.svelte.js";
  import { t } from "../i18n/i18n.svelte.js";

  const ws = $derived(workspace.pages);
  const desktop = $derived(ws ? pages.activeDesktop(ws) : null);
  const active = $derived(ws ? pages.activePage(ws) : null);
  const tabs = $derived(ws ? pages.openPages(ws) : []);
  const closed = $derived(ws ? pages.savedPages(ws) : []);

  let open = $state(null);
  /** What is being renamed: `{ kind: "page" | "desktop", id, name }`, or null. */
  let renaming = $state(null);
  /** The id of a desktop or closed page whose removal awaits a second click. */
  let confirming = $state(null);
  let bar = $state(null);
  const id = $props.id();

  function toggle(menu) {
    open = open === menu ? null : menu;
    confirming = null;
  }

  function close() {
    open = null;
    confirming = null;
  }

  function onWindowPointerDown(event) {
    if (open && !bar?.contains(event.target)) close();
  }

  function onKeydown(event) {
    if (event.key === "Escape") close();
  }

  function startRename(kind, id, name) {
    close();
    renaming = { kind, id, name };
  }

  function finishRename(keep) {
    if (!renaming) return;
    const { kind, id, name } = renaming;
    renaming = null;
    if (!keep) return;
    if (kind === "page") renamePage(id, name);
    else renameDesktop(id, name);
  }

  function focusSelect(node) {
    node.focus();
    node.select();
  }

  /** Runs `act` on the second click of the same item; the first only asks. */
  function twice(id, act) {
    if (confirming !== id) {
      confirming = id;
      return;
    }
    confirming = null;
    act();
  }

  const run = (act) => () => {
    act();
    close();
  };
</script>

<svelte:window onpointerdown={onWindowPointerDown} onkeydown={onKeydown} />

{#snippet nameField()}
  <input
    class="name"
    bind:value={renaming.name}
    aria-label={renaming.kind === "page" ? t("workspace.pageNameLabel") : t("workspace.desktopNameLabel")}
    use:focusSelect
    onkeydown={(event) => {
      if (event.key === "Enter") finishRename(true);
      if (event.key === "Escape") finishRename(false);
    }}
    onblur={() => finishRename(true)}
  />
{/snippet}

{#if ws}
  <nav class="pages" aria-label={t("workspace.pages")} bind:this={bar}>
    <div class="menu">
      {#if renaming?.kind === "desktop"}
        {@render nameField()}
      {:else}
        <button
          class="desktop"
          aria-expanded={open === "desktop"}
          aria-controls="{id}-desktops"
          onclick={() => toggle("desktop")}>{desktop.name} ▾</button
        >
      {/if}
      {#if open === "desktop"}
        <ul id="{id}-desktops" data-popup>
          {#each ws.desktops as each (each.id)}
            <li>
              <button aria-current={each.id === desktop.id ? "true" : undefined} onclick={run(() => goToDesktop(each.id))}>
                <span class="check" aria-hidden="true">{each.id === desktop.id ? "●" : ""}</span>{each.name}
              </button>
            </li>
          {/each}
          <li class="separator" aria-hidden="true"></li>
          <li><button onclick={run(newDesktop)}>{t("workspace.newDesktop")}</button></li>
          <li>
            <button onclick={() => startRename("desktop", desktop.id, desktop.name)}>{t("workspace.renameDesktop")}</button>
          </li>
          {#if ws.desktops.length > 1}
            <li>
              <button
               
                class:danger={confirming === desktop.id}
                onclick={() => twice(desktop.id, run(() => removeDesktop(desktop.id)))}
              >
                {confirming === desktop.id
                  ? t("workspace.confirmRemoveDesktop", { name: desktop.name })
                  : t("workspace.removeDesktop")}
              </button>
            </li>
          {/if}
        </ul>
      {/if}
    </div>

    <ul class="tabs" aria-label={t("workspace.openPages")}>
      {#each tabs as page (page.id)}
        <li class="tab" class:active={page.id === active.id}>
          {#if renaming?.kind === "page" && renaming.id === page.id}
            {@render nameField()}
          {:else}
            <button
              class="page"
              aria-current={page.id === active.id ? "page" : undefined}
              title={t("workspace.renameHint")}
              onclick={() => goToPage(page.id)}
              ondblclick={() => startRename("page", page.id, page.name)}>{page.name}</button
            >
            <button class="x" aria-label={t("workspace.closePage", { name: page.name })} onclick={() => closePageTab(page.id)}>×</button>
          {/if}
        </li>
      {/each}
    </ul>

    <div class="menu">
      <button
        class="add"
        aria-expanded={open === "add"}
        aria-controls="{id}-add"
        aria-label={t("workspace.newPage")}
        onclick={() => toggle("add")}>+</button
      >
      {#if open === "add"}
        <ul id="{id}-add" data-popup>
          <li><button onclick={run(() => newPage())}>{t("workspace.blankPage")}</button></li>
          <li><button onclick={run(copyPage)}>{t("workspace.copyPage")}</button></li>
          <li class="heading">{t("workspace.fromLayout")}</li>
          {#each listLayouts() as layout (layout.id)}
            <li><button onclick={run(() => newPage(layout.id))}>{t(layout.title)}</button></li>
          {/each}
          {#if closed.length}
            <li class="heading">{t("workspace.closedPages")}</li>
            {#each closed as page (page.id)}
              <li class="row">
                <button onclick={run(() => reopenPage(page.id))}>{page.name}</button>
                <button
                  class="delete"
                  class:danger={confirming === page.id}
                  onclick={() => twice(page.id, () => deletePage(page.id))}
                >
                  {confirming === page.id ? t("workspace.confirmDeletePage", { name: page.name }) : t("workspace.deletePage")}
                </button>
              </li>
            {/each}
          {/if}
        </ul>
      {/if}
    </div>
  </nav>
{/if}

<style>
  .pages {
    display: flex;
    align-items: stretch;
    gap: 0.375rem;
    min-height: 1.875rem;
    padding: 0 0.5rem;
    background: var(--panel);
    border-bottom: 1px solid var(--rule);
  }

  .menu {
    position: relative;
    display: flex;
    align-items: center;
  }

  .tabs {
    display: flex;
    margin: 0;
    padding: 0;
    list-style: none;
    align-items: stretch;
    gap: 0.125rem;
    min-width: 0;
    overflow-x: auto;
  }

  .tab {
    display: flex;
    align-items: center;
    border-bottom: 2px solid transparent;
  }

  .tab.active {
    border-bottom-color: var(--accent);
  }

  button {
    font: inherit;
    font-size: var(--text-sm);
    background: transparent;
    border: none;
    color: var(--text-dim);
    cursor: pointer;
  }

  .tab > .page {
    padding: 0.25rem 0.5rem;
    white-space: nowrap;
  }

  .tab.active > .page {
    color: var(--text);
  }

  .x {
    padding: 0.125rem 0.375rem;
    opacity: 0.6;
  }

  .desktop,
  .add {
    padding: 0.1875rem 0.625rem;
    border: 1px solid var(--rule);
  }

  .add {
    padding: 0.125rem 0.5625rem;
    font-size: var(--text-lg);
  }

  button:hover,
  button:focus-visible {
    color: var(--text);
    outline: none;
  }

  .x:hover,
  .x:focus-visible {
    opacity: 1;
    color: var(--down);
  }

  .name {
    width: 9.375rem;
    padding: 0.1875rem 0.5rem;
    font: inherit;
    font-size: var(--text-sm);
    background: var(--ink);
    border: 1px solid var(--accent);
    color: var(--text);
    outline: none;
  }

  /* The drop-down menus only: the page tabs are a list too. */
  [data-popup] {
    position: absolute;
    top: calc(100% + 0.125rem);
    left: 0;
    z-index: 1000;
    min-width: 15rem;
    margin: 0;
    padding: 0.25rem;
    list-style: none;
    background: var(--panel-raised);
    border: 1px solid var(--rule);
    box-shadow: var(--shadow-menu);
  }

  [data-popup] li > button {
    width: 100%;
    padding: 0.4375rem 0.625rem;
    text-align: left;
    color: var(--text);
  }

  [data-popup] li > button:hover,
  [data-popup] li > button:focus-visible {
    background: color-mix(in srgb, var(--accent) 12%, transparent);
  }

  li.row {
    display: flex;
  }

  li.row > button:first-child {
    flex: 1;
  }

  .delete {
    width: auto;
    padding: 0.4375rem 0.625rem;
    color: var(--text-dim);
    white-space: nowrap;
  }

  .danger {
    color: var(--down);
  }

  .check {
    display: inline-block;
    width: 1rem;
    color: var(--accent);
  }

  li.separator {
    margin: 0.25rem 0;
    border-top: 1px solid var(--rule);
  }

  .heading {
    padding: 0.5rem 0.625rem 0.25rem;
    font-size: var(--text-2xs);
    letter-spacing: 0.1em;
    text-transform: uppercase;
    color: var(--text-dim);
  }
</style>
