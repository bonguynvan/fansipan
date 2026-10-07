<script>
  import { onMount, mount, unmount } from "svelte";
  // The UMD build: in dockview-core 8 only it carries the library's CSS
  // (injected on load); the ES module build ships none.
  import { createDockview, themeDark } from "dockview-core/dist/dockview-core.js";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { getScreen } from "./registry.js";
  import { createRenderer } from "./renderer.svelte.js";
  import GroupActions from "./GroupActions.svelte";
  import { MAIN_WINDOW, persist, popOut, restore, retitlePanels, workspace } from "./workspace.svelte.js";
  import { i18n, t } from "../i18n/i18n.svelte.js";

  let { label } = $props();

  let host;

  // dockview's dark theme with the app's palette on top (see app.css).
  const theme = { ...themeDark, name: "fansipan", className: `${themeDark.className} dockview-theme-fansipan` };

  function headerActions(group) {
    const element = document.createElement("div");
    element.className = "group-actions";
    let instance = null;
    return {
      element,
      init() {
        instance = mount(GroupActions, { target: element, props: { group } });
      },
      dispose() {
        if (instance) unmount(instance);
        instance = null;
      },
    };
  }

  onMount(() => {
    const api = createDockview(host, {
      theme,
      // Panels leave the main window as native windows, not as dockview's
      // floating groups or browser pop-ups.
      disableFloatingGroups: true,
      createComponent: ({ name }) => createRenderer(getScreen(name), name),
      createRightHeaderActionComponent: headerActions,
      getTabContextMenuItems: ({ panel }) => [
        "maximize",
        { label: t("workspace.moveToNewWindow"), action: () => popOut(panel) },
        "separator",
        "close",
        "closeOthers",
      ],
    });

    const subscriptions = [api.onDidLayoutChange(persist)];
    // An extra window with nothing left in it has no reason to stay open.
    if (label !== MAIN_WINDOW) {
      subscriptions.push(
        api.onDidRemovePanel(() => {
          if (api.totalPanels === 0 && !workspace.rebuilding) getCurrentWindow().close();
        }),
      );
    }
    restore(api, label);

    return () => {
      subscriptions.forEach((s) => s.dispose());
      api.dispose();
      workspace.api = null;
    };
  });

  $effect(() => {
    if (i18n.locale) retitlePanels();
  });
</script>

<div class="root">
  {#if workspace.notice}
    <p class="notice" role="status">
      {t(workspace.notice.id, workspace.notice.vars)}
      <button onclick={() => (workspace.notice = null)} aria-label={t("workspace.dismiss")}>×</button>
    </p>
  {/if}

  <div class="workspace" bind:this={host}></div>
</div>

<style>
  /* The dock always takes the flexible row, notice or not. */
  .root {
    display: grid;
    grid-template-rows: auto minmax(0, 1fr);
    height: 100%;
    min-height: 0;
  }

  .workspace {
    grid-row: 2;
    min-height: 0;
  }

  .notice {
    display: flex;
    justify-content: space-between;
    gap: 12px;
    margin: 0;
    padding: 6px 12px;
    font-size: 12px;
    color: var(--accent);
    background: color-mix(in srgb, var(--accent) 10%, var(--panel));
    border-bottom: 1px solid var(--rule);
  }

  .notice button {
    background: none;
    border: none;
    color: inherit;
    cursor: pointer;
  }
</style>
