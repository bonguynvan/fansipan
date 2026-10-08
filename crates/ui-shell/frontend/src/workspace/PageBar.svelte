<script>
  // The main window's desktops and pages (ADR 0013): a desktop menu, the
  // open pages as tabs, and "+" for a new page (blank, from a registered
  // layout, or a copy of this one) or a closed page to open again.
  // Double-click a tab to rename it. Removing a desktop or deleting a closed
  // page asks once more on the same button.
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
        <button class="desktop" aria-expanded={open === "desktop"} onclick={() => toggle("desktop")}>{desktop.name} ▾</button>
      {/if}
      {#if open === "desktop"}
        <ul role="menu">
          {#each ws.desktops as each (each.id)}
            <li role="none">
              <button role="menuitemradio" aria-checked={each.id === desktop.id} onclick={run(() => goToDesktop(each.id))}>
                <span class="check" aria-hidden="true">{each.id === desktop.id ? "●" : ""}</span>{each.name}
              </button>
            </li>
          {/each}
          <li role="separator"></li>
          <li role="none"><button role="menuitem" onclick={run(newDesktop)}>{t("workspace.newDesktop")}</button></li>
          <li role="none">
            <button role="menuitem" onclick={() => startRename("desktop", desktop.id, desktop.name)}>{t("workspace.renameDesktop")}</button>
          </li>
          {#if ws.desktops.length > 1}
            <li role="none">
              <button
                role="menuitem"
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

    <div class="tabs" role="tablist" aria-label={t("workspace.openPages")}>
      {#each tabs as page (page.id)}
        <div class="tab" class:active={page.id === active.id}>
          {#if renaming?.kind === "page" && renaming.id === page.id}
            {@render nameField()}
          {:else}
            <button
              role="tab"
              aria-selected={page.id === active.id}
              title={t("workspace.renameHint")}
              onclick={() => goToPage(page.id)}
              ondblclick={() => startRename("page", page.id, page.name)}>{page.name}</button
            >
            <button class="x" aria-label={t("workspace.closePage", { name: page.name })} onclick={() => closePageTab(page.id)}>×</button>
          {/if}
        </div>
      {/each}
    </div>

    <div class="menu">
      <button class="add" aria-expanded={open === "add"} aria-label={t("workspace.newPage")} onclick={() => toggle("add")}>+</button>
      {#if open === "add"}
        <ul role="menu">
          <li role="none"><button role="menuitem" onclick={run(() => newPage())}>{t("workspace.blankPage")}</button></li>
          <li role="none"><button role="menuitem" onclick={run(copyPage)}>{t("workspace.copyPage")}</button></li>
          <li role="presentation" class="heading">{t("workspace.fromLayout")}</li>
          {#each listLayouts() as layout (layout.id)}
            <li role="none"><button role="menuitem" onclick={run(() => newPage(layout.id))}>{t(layout.title)}</button></li>
          {/each}
          {#if closed.length}
            <li role="presentation" class="heading">{t("workspace.closedPages")}</li>
            {#each closed as page (page.id)}
              <li role="none" class="row">
                <button role="menuitem" onclick={run(() => reopenPage(page.id))}>{page.name}</button>
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
    gap: 6px;
    min-height: 30px;
    padding: 0 8px;
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
    align-items: stretch;
    gap: 2px;
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
    font-size: 12px;
    background: transparent;
    border: none;
    color: var(--text-dim);
    cursor: pointer;
  }

  .tab > button[role="tab"] {
    padding: 4px 8px;
    white-space: nowrap;
  }

  .tab.active > button[role="tab"] {
    color: var(--text);
  }

  .x {
    padding: 2px 6px;
    opacity: 0.6;
  }

  .desktop,
  .add {
    padding: 3px 10px;
    border: 1px solid var(--rule);
  }

  .add {
    padding: 2px 9px;
    font-size: 14px;
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
    width: 150px;
    padding: 3px 8px;
    font: inherit;
    font-size: 12px;
    background: var(--ink);
    border: 1px solid var(--accent);
    color: var(--text);
    outline: none;
  }

  ul {
    position: absolute;
    top: calc(100% + 2px);
    left: 0;
    z-index: 1000;
    min-width: 240px;
    margin: 0;
    padding: 4px;
    list-style: none;
    background: var(--panel-raised);
    border: 1px solid var(--rule);
    box-shadow: 0 10px 30px rgb(0 0 0 / 45%);
  }

  li > button {
    width: 100%;
    padding: 7px 10px;
    text-align: left;
    color: var(--text);
  }

  li > button:hover,
  li > button:focus-visible {
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
    padding: 7px 10px;
    color: var(--text-dim);
    white-space: nowrap;
  }

  .danger {
    color: var(--down);
  }

  .check {
    display: inline-block;
    width: 16px;
    color: var(--accent);
  }

  li[role="separator"] {
    margin: 4px 0;
    border-top: 1px solid var(--rule);
  }

  .heading {
    padding: 8px 10px 4px;
    font-size: 10px;
    letter-spacing: 0.1em;
    text-transform: uppercase;
    color: var(--text-dim);
  }
</style>
