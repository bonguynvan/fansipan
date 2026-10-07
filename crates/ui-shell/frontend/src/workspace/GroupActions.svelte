<script>
  // The buttons at the right of each panel group's tab strip: the active
  // panel's link group, and moving it to a window of its own.
  import { cycleLink, popOut, screenOf } from "./workspace.svelte.js";
  import { LINK_GROUPS, groupColor, isLinked } from "../lib/links.svelte.js";
  import { t } from "../i18n/i18n.svelte.js";

  let { group } = $props();

  let panel = $state.raw(null);
  let link = $state(undefined);

  function watchParams(target) {
    return target?.api.onDidParametersChange((params) => (link = params.link)) ?? { dispose() {} };
  }

  // Follow the group's active panel, and that panel's link parameter.
  $effect(() => {
    const show = (target) => {
      panel = target ?? null;
      link = target?.params?.link;
    };
    show(group.activePanel);
    let params = watchParams(group.activePanel);
    const active = group.api.onDidActivePanelChange((event) => {
      show(event.panel);
      params.dispose();
      params = watchParams(event.panel);
    });
    return () => {
      active.dispose();
      params.dispose();
    };
  });

  const linkable = $derived(screenOf(panel)?.linkable ?? false);
  const linkLabel = $derived(isLinked(link) ? link.toUpperCase() : "–");
</script>

<div class="actions">
  {#if panel && linkable}
    <button
      class="link"
      style:--group={groupColor(link)}
      title={isLinked(link)
        ? t("workspace.linkGroup", { group: linkLabel })
        : t("workspace.notLinked", { group: LINK_GROUPS[0].label })}
      aria-label={t("workspace.changeLink", { group: linkLabel })}
      onclick={() => cycleLink(panel)}>{linkLabel}</button
    >
  {/if}
  {#if panel}
    <button
      class="popout"
      title={t("workspace.moveToNewWindow")}
      aria-label={t("workspace.moveToNewWindowNamed", { title: panel.title })}
      onclick={() => popOut(panel)}>⧉</button
    >
  {/if}
</div>

<style>
  .actions {
    display: flex;
    align-items: center;
    gap: 4px;
    height: 100%;
    padding: 0 6px;
  }

  button {
    min-width: 22px;
    height: 20px;
    padding: 0 6px;
    font: 600 11px/1 var(--mono);
    background: transparent;
    border: 1px solid var(--rule);
    color: var(--text-dim);
    cursor: pointer;
  }

  button:hover,
  button:focus-visible {
    color: var(--text);
    border-color: var(--text-dim);
    outline: none;
  }

  .link {
    color: var(--group);
    border-color: color-mix(in srgb, var(--group) 55%, transparent);
  }
</style>
