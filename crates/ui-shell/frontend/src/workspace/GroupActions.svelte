<script>
  // The buttons at the right of each panel group's tab strip: the active
  // panel's link group, and moving it to a window of its own.
  import { cycleLink, popOut, screenOf } from "./workspace.svelte.js";
  import { groupColor, isLinked } from "../lib/links.svelte.js";

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
      title={isLinked(link) ? `Link group ${linkLabel}: click for the next group` : "Not linked: click to link to A"}
      aria-label="Link group {linkLabel}, change"
      onclick={() => cycleLink(panel)}>{linkLabel}</button
    >
  {/if}
  {#if panel}
    <button class="popout" title="Move to a new window" aria-label="Move {panel.title} to a new window" onclick={() => popOut(panel)}>⧉</button>
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
