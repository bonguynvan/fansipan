<script>
  // A compact select for the top bar's app-wide settings (text size,
  // language, and the like), labelled for screen readers and on hover.
  // `options` is a list of `{ value, text }`; `onchange` gets the chosen
  // option's value as given, not as a string.
  let { value, label, options, onchange } = $props();
</script>

<select
  class="tool"
  {value}
  aria-label={label}
  title={label}
  onchange={(event) => onchange(options[event.currentTarget.selectedIndex].value)}
>
  {#each options as option (option.value)}
    <option value={option.value}>{option.text}</option>
  {/each}
</select>

<style>
  .tool {
    padding: 0.1875rem 0.375rem;
    font-size: var(--text-xs);
    letter-spacing: 0.04em;
    background: transparent;
    border: 1px solid var(--rule);
    color: var(--text-dim);
    cursor: pointer;
  }

  .tool:hover,
  .tool:focus-visible {
    color: var(--text);
    border-color: var(--text-dim);
    outline: none;
  }

  option {
    background: var(--panel);
    color: var(--text);
  }
</style>
