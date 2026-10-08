<script>
  // Every keyboard action and its key, and the place to change them. Click
  // Change, press the new keys: Esc calls it off, Backspace or Delete
  // leaves the action without a key. A key already in use moves here, and
  // the dialog says which action lost it. Only F-keys and combinations
  // with Ctrl or Alt are taken: plain keys belong to fields and buttons.
  import { bindKey, hotkeys, resetKeys } from "./hotkeys.svelte.js";
  import { actionFor, comboOf } from "./hotkeys.js";
  import { t } from "../i18n/i18n.svelte.js";

  let { open = $bindable(false) } = $props();

  let dialog;
  /** The action waiting for its new key, or null. */
  let recordingFor = $state(null);
  let note = $state(null);

  $effect(() => {
    if (!dialog) return;
    if (open && !dialog.open) dialog.showModal();
    if (!open && dialog.open) dialog.close();
  });

  // While a key is being recorded, presses must not run the actions they
  // happen to be bound to.
  $effect(() => {
    hotkeys.recording = recordingFor !== null;
  });

  const titleOf = (id) => {
    const action = hotkeys.actions.find((a) => a.id === id);
    return action ? t(action.title) : id;
  };

  function record(id) {
    note = null;
    recordingFor = id;
  }

  function onKeydown(event) {
    if (recordingFor === null) return;
    event.preventDefault();
    event.stopPropagation();
    if (event.key === "Escape") {
      recordingFor = null;
      return;
    }
    if (event.key === "Backspace" || event.key === "Delete") {
      bindKey(recordingFor, null);
      recordingFor = null;
      return;
    }
    const combo = comboOf(event);
    if (!combo) return;
    const previous = actionFor(hotkeys.bindings, combo);
    if (!bindKey(recordingFor, combo)) {
      note = t("hotkey.needsModifier", { key: combo });
      return;
    }
    note =
      previous && previous !== recordingFor
        ? t("hotkey.moved", { key: combo, action: titleOf(previous) })
        : null;
    recordingFor = null;
  }

  function close() {
    recordingFor = null;
    note = null;
    open = false;
  }
</script>

<dialog
  bind:this={dialog}
  class="keys"
  aria-labelledby="keys-title"
  onkeydown={onKeydown}
  oncancel={(event) => {
    // Esc while recording only calls the recording off.
    event.preventDefault();
    if (recordingFor === null) close();
  }}
  onclose={close}
>
  <h2 id="keys-title">{t("hotkey.title")}</h2>
  <p class="dim intro">{t("hotkey.intro")}</p>

  <table>
    <tbody>
      {#each hotkeys.actions as action (action.id)}
        <tr>
          <th scope="row">{t(action.title)}</th>
          <td>
            {#if recordingFor === action.id}
              <span class="recording" role="status">{t("hotkey.press")}</span>
            {:else if hotkeys.bindings[action.id]}
              <kbd>{hotkeys.bindings[action.id]}</kbd>
            {:else}
              <span class="dim">{t("hotkey.none")}</span>
            {/if}
          </td>
          <td class="change">
            <button onclick={() => record(action.id)} disabled={recordingFor !== null}>{t("hotkey.change")}</button>
          </td>
        </tr>
      {/each}
    </tbody>
  </table>

  {#if note}
    <p class="note" role="status">{note}</p>
  {/if}

  <footer>
    <button
      onclick={() => {
        resetKeys();
        note = null;
      }}
      disabled={recordingFor !== null}>{t("hotkey.reset")}</button
    >
    <button class="primary" onclick={close}>{t("hotkey.close")}</button>
  </footer>
</dialog>

<style>
  .keys {
    width: min(32.5rem, calc(100vw - 2rem));
    padding: 1.125rem 1.25rem 1rem;
    background: var(--panel);
    color: var(--text);
    border: 1px solid var(--rule);
    border-radius: 0;
  }

  .keys::backdrop {
    background: rgb(0 0 0 / 0.55);
  }

  h2 {
    margin: 0 0 0.25rem;
    font-size: var(--text-xl);
  }

  .intro {
    margin: 0 0 0.75rem;
    font-size: var(--text-sm);
  }

  table {
    width: 100%;
    border-collapse: collapse;
    font-size: var(--text-md);
  }

  th {
    text-align: left;
    font-weight: 400;
  }

  th,
  td {
    padding: 0.375rem 0;
    border-bottom: 1px solid var(--rule);
  }

  td {
    padding-left: 0.75rem;
  }

  .change {
    text-align: right;
  }

  kbd {
    padding: 0.0625rem 0.375rem;
    font-family: var(--mono);
    font-size: var(--text-sm);
    background: var(--ink);
    border: 1px solid var(--rule);
  }

  .recording {
    color: var(--accent);
  }

  .note {
    margin: 0.625rem 0 0;
    font-size: var(--text-sm);
    color: var(--accent);
  }

  footer {
    display: flex;
    justify-content: flex-end;
    gap: 0.5rem;
    margin-top: 0.875rem;
  }

  button {
    padding: 0.25rem 0.75rem;
    font: inherit;
    font-size: var(--text-xs);
    text-transform: uppercase;
    letter-spacing: 0.06em;
    background: transparent;
    border: 1px solid var(--rule);
    color: var(--text-dim);
    cursor: pointer;
  }

  button:hover:not(:disabled),
  button:focus-visible {
    color: var(--text);
    border-color: var(--accent);
    outline: none;
  }

  button:disabled {
    opacity: 0.5;
    cursor: default;
  }

  button.primary {
    color: var(--ink);
    background: var(--accent);
    border-color: var(--accent);
  }
</style>
