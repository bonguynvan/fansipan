<script>
  // "Add panel", filled from the screen registry, so a screen a plugin
  // registers shows up here with no change to this file. Registered layouts
  // open as new pages from the page bar (PageBar.svelte).
  import { listScreens } from "./registry.js";
  import { addScreen } from "./workspace.svelte.js";
  import { t } from "../i18n/i18n.svelte.js";

  let open = $state(null);
  let nav;

  function toggle(menu) {
    open = open === menu ? null : menu;
  }

  function close() {
    open = null;
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

</nav>

<style>
  .workspace-menu {
    display: flex;
    gap: 0.375rem;
  }

  .menu {
    position: relative;
  }

  .menu > button {
    padding: 0.25rem 0.625rem;
    font-size: var(--text-xs);
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
    top: calc(100% + 0.25rem);
    right: 0;
    z-index: 1000;
    min-width: 17.5rem;
    margin: 0;
    padding: 0.25rem;
    list-style: none;
    background: var(--panel-raised);
    border: 1px solid var(--rule);
    box-shadow: var(--shadow-menu);
  }

  li > button {
    display: grid;
    gap: 0.125rem;
    width: 100%;
    padding: 0.5rem 0.625rem;
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
    font-size: var(--text-md);
  }

  .desc {
    font-size: var(--text-xs);
  }
</style>
