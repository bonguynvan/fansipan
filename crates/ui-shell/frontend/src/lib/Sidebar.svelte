<script>
  // The sidebar of places (ADR 0017): one button a section, Trade first,
  // the section shown marked. The arrows walk it, as they do a toolbar.
  import { keyOf } from "./hotkeys.svelte.js";
  import { listSections, place, showSection } from "./sections.svelte.js";
  import { t } from "../i18n/i18n.svelte.js";

  const sections = listSections();
  const buttons = [];

  function onKeydown(event, index) {
    const last = sections.length - 1;
    const to = { ArrowDown: (index + 1) % sections.length, ArrowUp: (index + last) % sections.length, Home: 0, End: last }[
      event.key
    ];
    if (to === undefined) return;
    event.preventDefault();
    buttons[to]?.focus();
  }

  const titleOf = (section) => {
    const key = keyOf(`section.${section.id}`);
    return key ? `${t(section.title)} (${key})` : t(section.title);
  };
</script>

<nav class="sidebar" aria-label={t("section.bar")}>
  {#each sections as section, index (section.id)}
    <button
      bind:this={buttons[index]}
      class:on={place.current === section.id}
      aria-current={place.current === section.id ? "page" : undefined}
      aria-label={t(section.title)}
      title={titleOf(section)}
      tabindex={place.current === section.id ? 0 : -1}
      onclick={() => showSection(section.id)}
      onkeydown={(event) => onKeydown(event, index)}
    >
      <svg viewBox="0 0 24 24" aria-hidden="true"><path d={section.icon} /></svg>
      <span>{t(section.title)}</span>
    </button>
  {/each}
</nav>

<style>
  .sidebar {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
    width: 4.25rem;
    padding: 0.5rem 0;
    background: var(--ink);
    border-right: 1px solid var(--rule);
  }

  button {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 0.25rem;
    margin: 0 0.375rem;
    padding: 0.5rem 0.125rem;
    color: var(--text-dim);
    background: transparent;
    border: none;
    border-left: 2px solid transparent;
    cursor: pointer;
    min-width: 0;
  }

  svg {
    width: 1.25rem;
    height: 1.25rem;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.5;
    stroke-linecap: round;
    stroke-linejoin: round;
  }

  span {
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--text-xs);
  }

  button:hover,
  button:focus-visible {
    color: var(--text);
    background: color-mix(in srgb, var(--text) 8%, transparent);
    outline: none;
  }

  button.on {
    color: var(--accent);
    border-left-color: var(--accent);
  }
</style>
