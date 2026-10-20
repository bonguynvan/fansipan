<script>
  import { onMount, mount, unmount } from "svelte";
  // The UMD build: in dockview-core 8 only it carries the library's CSS
  // (injected on load); the ES module build ships none.
  import { createDockview, themeDark, themeLight } from "dockview-core/dist/dockview-core.js";
  import { getScreen } from "./registry.js";
  import { createRenderer } from "./renderer.svelte.js";
  import GroupActions from "./GroupActions.svelte";
  import PageBar from "./PageBar.svelte";
  import { movePanelToPage, otherPages, persist, restore, retitlePanels, workspace } from "./workspace.svelte.js";
  import { i18n, t, tn } from "../i18n/i18n.svelte.js";
  import { theme } from "../lib/theme.svelte.js";

  let host;

  // dockview's dark or light theme, matching the app's, with the
  // app's palette on top (see app.css).
  const dockTheme = (resolved) => {
    const base = resolved === "light" ? themeLight : themeDark;
    return { ...base, name: "fansipan", className: `${base.className} dockview-theme-fansipan` };
  };

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
      theme: dockTheme(theme.resolved),
      // Every panel is docked: dragged to split, stack, or resize, never
      // lifted over the page.
      disableFloatingGroups: true,
      createComponent: ({ name }) => createRenderer(getScreen(name), name),
      createRightHeaderActionComponent: headerActions,
      getTabContextMenuItems: ({ panel }) => [
        "maximize",
        ...otherPages().map((page) => ({
          label: t("workspace.moveToPage", { name: page.name }),
          action: () => movePanelToPage(panel, page.id),
        })),
        "separator",
        "close",
        "closeOthers",
      ],
    });

    const subscriptions = [api.onDidLayoutChange(persist)];
    restore(api);

    return () => {
      subscriptions.forEach((s) => s.dispose());
      api.dispose();
      workspace.api = null;
    };
  });

  $effect(() => {
    if (i18n.locale) retitlePanels();
  });

  $effect(() => {
    workspace.api?.updateOptions({ theme: dockTheme(theme.resolved) });
  });
</script>

<div class="root">
  {#if workspace.notice}
    <p class="notice" role="status">
      {workspace.notice.count === undefined
        ? t(workspace.notice.id, workspace.notice.vars)
        : tn(workspace.notice.id, workspace.notice.count, workspace.notice.vars)}
      <button onclick={() => (workspace.notice = null)} aria-label={t("workspace.dismiss")}>×</button>
    </p>
  {/if}

  <PageBar />

  <div class="workspace" bind:this={host}></div>
</div>

<style>
  /* The dock always takes the flexible row, whether the notice and the page bar show or not. */
  .root {
    display: grid;
    grid-template-rows: auto auto minmax(0, 1fr);
    height: 100%;
    min-height: 0;
  }

  .workspace {
    grid-row: 3;
    min-height: 0;
  }

  .notice {
    display: flex;
    justify-content: space-between;
    gap: 0.75rem;
    margin: 0;
    padding: 0.375rem 0.75rem;
    font-size: var(--text-sm);
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
