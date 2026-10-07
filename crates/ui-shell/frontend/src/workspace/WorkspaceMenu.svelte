<script>
  // "Add panel" and "Layout" menus, filled from the registries, so a screen
  // or layout a plugin registers shows up here with no change to this file.
  import { listLayouts, listScreens } from "./registry.js";
  import { addScreen, applyLayout } from "./workspace.svelte.js";
  import { t } from "../i18n/i18n.svelte.js";

  let open = $state(null);
  let pendingLayout = $state(null);
  let nav;

  function toggle(menu) {
    open = open === menu ? null : menu;
    pendingLayout = null;
  }

  function close() {
    open = null;
    pendingLayout = null;
  }

  // Pointer-down, not click: it fires before any menu item's click handler
  // re-renders the menu, so "was this inside the menu?" is still answerable.
  function onWindowPointerDown(event) {
    if (open && !nav?.contains(event.target)) close();
  }

  function onKeydown(event) {
    if (event.key === "Escape") close();
  }
</script>

<svelte:window onpointerdown={onWindowPointerDown} onkeydown={onKeydown} />

<nav class="workspace-menu" aria-label={t("workspace.menu")} bind:this={nav}>
  <div class="menu">
    <button aria-expanded={open === "add"} onclick={() => toggle("add")}>{t("workspace.addPanel")} ▾</button>
    {#if open === "add"}
      <ul role="menu">
        {#each listScreens() as screen (screen.id)}
          <li role="none">
            <button
              role="menuitem"
              onclick={() => {
                addScreen(screen.id);
                close();
              }}
            >
              <span class="title">{t(screen.title)}</span>
              {#if screen.description}<span class="desc dim">{t(screen.description)}</span>{/if}
            </button>
          </li>
        {/each}
      </ul>
    {/if}
  </div>

  <div class="menu">
    <button aria-expanded={open === "layout"} onclick={() => toggle("layout")}>{t("workspace.layout")} ▾</button>
    {#if open === "layout"}
      <ul role="menu">
        {#each listLayouts() as layout (layout.id)}
          <li role="none">
            {#if pendingLayout === layout.id}
              <div class="confirm">
                <span>{t("workspace.replaceConfirm")}</span>
                <button
                  class="danger"
                  onclick={() => {
                    applyLayout(layout.id);
                    close();
                  }}>{t("workspace.replace")}</button
                >
                <button onclick={() => (pendingLayout = null)}>{t("workspace.keep")}</button>
              </div>
            {:else}
              <button role="menuitem" onclick={() => (pendingLayout = layout.id)}>
                <span class="title">{t(layout.title)}</span>
              </button>
            {/if}
          </li>
        {/each}
      </ul>
    {/if}
  </div>
</nav>

<style>
  .workspace-menu {
    display: flex;
    gap: 6px;
  }

  .menu {
    position: relative;
  }

  .menu > button {
    padding: 4px 10px;
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    background: transparent;
    border: 1px solid var(--rule);
    color: var(--text-dim);
    cursor: pointer;
  }

  .menu > button:hover,
  .menu > button:focus-visible,
  .menu > button[aria-expanded="true"] {
    color: var(--text);
    border-color: var(--text-dim);
    outline: none;
  }

  ul {
    position: absolute;
    top: calc(100% + 4px);
    right: 0;
    z-index: 1000;
    min-width: 280px;
    margin: 0;
    padding: 4px;
    list-style: none;
    background: var(--panel-raised);
    border: 1px solid var(--rule);
    box-shadow: 0 10px 30px rgb(0 0 0 / 45%);
  }

  li > button {
    display: grid;
    gap: 2px;
    width: 100%;
    padding: 8px 10px;
    text-align: left;
    background: transparent;
    border: none;
    color: var(--text);
    cursor: pointer;
  }

  li > button:hover,
  li > button:focus-visible {
    background: color-mix(in srgb, var(--accent) 12%, transparent);
    outline: none;
  }

  .title {
    font-size: 13px;
  }

  .desc {
    font-size: 11px;
  }

  .confirm {
    display: flex;
    gap: 8px;
    align-items: center;
    padding: 8px 10px;
    font-size: 12px;
  }

  .confirm button {
    padding: 3px 10px;
    font-size: 11px;
    background: transparent;
    border: 1px solid var(--rule);
    color: var(--text);
    cursor: pointer;
  }

  .confirm .danger {
    color: var(--down);
    border-color: var(--down);
  }
</style>
