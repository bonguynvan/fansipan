<script>
  // The first launch of a build that offers several venues: the trader
  // picks one before anything connects. Nothing is picked for them.
  import { onMount } from "svelte";
  import { chooseVenue, launch } from "./launch.svelte.js";
  import { startLocale, t } from "../i18n/i18n.svelte.js";
  import { startTheme } from "./theme.svelte.js";
  import { startUiScale } from "./ui.svelte.js";

  /** Called once the chosen venue is running. */
  let { onChosen } = $props();

  let busy = $state(null);
  // Why the remembered venue, if any, did not open.
  let error = $state(launch.problem ? t("venue.failed", { error: launch.problem }) : "");

  onMount(() => {
    const failed = (err) => (error = String(err));
    startLocale()
      .then(() => {
        if (launch.problem) error = t("venue.failed", { error: launch.problem });
      })
      .catch(failed);
    startTheme().catch(failed);
    startUiScale().catch(failed);
  });

  async function pick(id) {
    if (busy) return;
    busy = id;
    error = "";
    try {
      await chooseVenue(id);
      onChosen();
    } catch (err) {
      error = t("venue.failed", { error: String(err) });
      busy = null;
    }
  }
</script>

<main class="chooser">
  <section aria-labelledby="venue-title">
    <h1 id="venue-title">{t("venue.title")}</h1>
    <p class="dim">{t("venue.intro")}</p>
    <ul>
      {#each launch.choices as choice (choice.id)}
        <li>
          <button
            class:busy={busy === choice.id}
            disabled={busy !== null}
            aria-busy={busy === choice.id}
            onclick={() => pick(choice.id)}
          >
            {choice.title}
          </button>
        </li>
      {/each}
    </ul>
    {#if error}<p class="error" role="alert">{error}</p>{/if}
    <p class="dim later">{t("venue.later")}</p>
  </section>
</main>

<style>
  .chooser {
    display: grid;
    place-items: center;
    min-height: 100vh;
    overflow: auto;
    padding: 1rem;
    background: var(--ink);
    color: var(--text);
  }

  section {
    width: min(28rem, 100%);
    padding: 1.25rem 1.5rem;
    background: var(--panel);
    border: 1px solid var(--rule);
  }

  h1 {
    margin: 0 0 0.375rem;
    font-size: var(--text-xl);
  }

  p {
    margin: 0 0 0.875rem;
    font-size: var(--text-md);
  }

  .dim {
    color: var(--text-dim);
  }

  .later {
    margin: 0.875rem 0 0;
    font-size: var(--text-sm);
  }

  ul {
    display: grid;
    gap: 0.5rem;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  button {
    width: 100%;
    min-width: 0;
    padding: 0.625rem 0.875rem;
    font-size: var(--text-md);
    text-align: start;
    color: var(--text);
    background: transparent;
    border: 1px solid var(--rule);
    cursor: pointer;
  }

  button:hover:not(:disabled) {
    border-color: var(--accent);
  }

  button:focus-visible {
    border-color: var(--accent);
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }

  button:disabled {
    cursor: default;
    opacity: 0.6;
  }

  button.busy {
    opacity: 1;
    border-color: var(--accent);
  }

  .error {
    margin: 0.875rem 0 0;
    color: var(--tone-down);
  }
</style>
