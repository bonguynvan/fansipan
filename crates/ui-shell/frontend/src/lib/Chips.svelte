<script>
  // A row of toggle chips with one pressed: a screen's filter, such as the
  // board's exchange. `options` is a list of `{ value, text }`; `onchange`
  // gets the chosen option's value.
  let { label, options, value, onchange } = $props();
</script>

<div class="chips" role="group" aria-label={label}>
  {#each options as option (option.value)}
    <button
      type="button"
      class:on={option.value === value}
      aria-pressed={option.value === value}
      onclick={() => onchange(option.value)}>{option.text}</button
    >
  {/each}
</div>

<style>
  .chips {
    display: flex;
    gap: 0.125rem;
  }

  button {
    padding: 0.1875rem 0.625rem;
    font: 600 var(--text-xs)/1.4 var(--mono);
    letter-spacing: 0.04em;
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

  button.on {
    color: var(--ink);
    background: var(--accent);
    border-color: var(--accent);
  }
</style>
