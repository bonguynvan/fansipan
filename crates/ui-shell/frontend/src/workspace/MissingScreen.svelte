<script>
  // Shown when a saved layout names a screen nobody registered, e.g. one
  // from a plugin that has since been removed. The rest of the layout loads.
  let { name } = $props();
</script>

<div class="missing">
  <p>No screen called <code>{name}</code> is installed.</p>
  <p class="dim">Close this panel, or add the screen back and restart.</p>
</div>

<style>
  .missing {
    padding: 16px;
    font-size: 12px;
  }

  p {
    margin: 0 0 6px;
  }

  code {
    font-family: var(--mono);
    color: var(--accent);
  }
</style>
