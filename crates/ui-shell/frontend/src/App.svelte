<script>
  import { onMount } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import { market, startMarket } from "./lib/market.svelte.js";
  import { connectionLines, summarize } from "./lib/connection.js";
  import { flushLinks, startLinks } from "./lib/links.svelte.js";
  import { startPicks } from "./lib/picks.svelte.js";
  import { flushWatchlists } from "./lib/watchlists.svelte.js";
  import { flushHotkeys, startHotkeys } from "./lib/hotkeys.svelte.js";
  import KeysDialog from "./lib/KeysDialog.svelte";
  import AboutDialog from "./lib/AboutDialog.svelte";
  import MenuBar from "./lib/MenuBar.svelte";
  import { registerAppMenus } from "./lib/appMenus.js";
  import { mark, money } from "./lib/format.js";
  import { journalNotice } from "./lib/journal.js";
  import { registerBuiltins } from "./workspace/builtins.js";
  import Workspace from "./workspace/Workspace.svelte";
  import {
    MAIN_WINDOW,
    currentWindowLabel,
    flushLayout,
    registerWorkspaceActions,
    workspace,
  } from "./workspace/workspace.svelte.js";
  import { flushLocale, startLocale, t } from "./i18n/i18n.svelte.js";
  import { startTheme } from "./lib/theme.svelte.js";
  import { startUiScale } from "./lib/ui.svelte.js";

  const label = currentWindowLabel();
  let keysOpen = $state(false);
  let aboutOpen = $state(false);

  // Screens must be registered before any layout that names them loads.
  registerBuiltins();
  registerWorkspaceActions();
  if (label === MAIN_WINDOW) {
    registerAppMenus({ showShortcuts: () => (keysOpen = true), showAbout: () => (aboutOpen = true) });
  }

  // A journal that stops being written is told at once (ADR 0023).
  $effect(() => {
    if (market.journalError && label === MAIN_WINDOW) {
      workspace.notice = { id: "journal.writeFailed", vars: { error: market.journalError.problem } };
    }
  });

  onMount(() => {
    const failed = (err) => (workspace.notice = { id: "app.startFailed", vars: { error: String(err) } });
    startLocale().catch(failed);
    startUiScale({
      onError: (error) => (workspace.notice = { id: "app.textSizeNotShared", vars: { error } }),
    }).catch(failed);
    startTheme({
      onError: (error) => (workspace.notice = { id: "app.themeNotShared", vars: { error } }),
    }).catch(failed);
    startLinks({
      onError: (error) => (workspace.notice = { id: "workspace.symbolNotShared", vars: { error } }),
    }).catch(failed);
    startPicks().catch(failed);
    startHotkeys({ onNotice: (notice) => (workspace.notice = notice) }).catch(failed);
    startMarket()
      .then(() => {
        // Told once, in the window the trader starts in.
        const notice = label === MAIN_WINDOW ? journalNotice(market.journal) : null;
        if (notice) workspace.notice = notice;
      })
      .catch(failed);
    // The shell sends this just before quitting and waits briefly, so saves
    // still inside their debounce window are not lost.
    const flushing = listen("workspace-flush", () => {
      flushLayout();
      flushLinks();
      flushLocale();
      flushWatchlists();
      flushHotkeys();
    });
    return () => flushing.then((unlisten) => unlisten());
  });

  const equity = $derived(
    Number(market.account.cash) +
      market.account.positions.reduce(
        (sum, p) => sum + Number(p.quantity) * mark(p, market.quotes[p.symbol]),
        0,
      ),
  );

  const signedMoney = (value) => `${Number(value) > 0 ? "+" : ""}${money(Number(value), market.info?.currency)}`;
  const dayHint = $derived(
    market.day
      ? t("app.dayHint", { costs: money(Number(market.day.costs), market.info?.currency) }) +
          (market.day.partial ? ` ${t("app.dayPartial")}` : "")
      : "",
  );

  const connectionText = ({ connection, state }) =>
    connection
      ? t(`app.connectionState.${state}`, { connection: t(`app.connection.${connection}`) })
      : t(`app.venueState.${state}`);
  const connection = $derived(summarize(market.connections));
  const status = $derived(
    !market.ready ? t("app.connecting") : connection.state === "up" ? t("app.live") : connectionText(connection),
  );
  // Every connection, with the adapter's own words when it gave any.
  const statusDetail = $derived(
    connectionLines(market.connections)
      .map((line) => connectionText(line) + (line.detail ? ` (${line.detail})` : ""))
      .join("\n"),
  );
</script>

<!-- An extra window is the screen alone: no top bar, moved by its tab strip. -->
<div class="terminal" class:detached={label !== MAIN_WINDOW}>
  {#if label === MAIN_WINDOW}
  <header>
    <div class="lead">
      <div class="brand">
        <span class="mark" aria-hidden="true">▲</span>
        <span>Fansipan</span>
        <span class="venue dim">{market.info?.title ?? ""}</span>
      </div>
      <MenuBar onNotice={(notice) => (workspace.notice = notice)} />
    </div>
    <div class="figures">
      <div class="equity">
        <span class="dim">{t("app.equity")}</span>
        <span class="num">{money(equity, market.info?.currency)}</span>
      </div>
      {#if market.day}
        <div class="day" title={dayHint}>
          <span class="dim">{t("app.day")}</span>
          <span class="num {Number(market.day.total) > 0 ? 'up' : Number(market.day.total) < 0 ? 'down' : ''}"
            >{market.day.partial ? "≈ " : ""}{signedMoney(market.day.total)}</span
          >
        </div>
      {/if}
    </div>
    <div class="right">
      {#if market.trading !== "active"}
        <span class="trading {market.trading}" role="status">{t(`trading.banner.${market.trading}`)}</span>
      {/if}
      <div class="status {market.ready ? connection.state : 'connecting'}" title={statusDetail} role="status">
        <span class="dot"></span>{status}
      </div>
    </div>
  </header>
  {/if}

  <main>
    <Workspace {label} />
  </main>
</div>

<KeysDialog bind:open={keysOpen} />
<AboutDialog bind:open={aboutOpen} />

<style>
  .terminal {
    display: grid;
    grid-template-rows: auto 1fr;
    height: 100vh;
  }

  /* No system frame around an extra window: a hairline keeps its edge. */
  .terminal.detached {
    grid-template-rows: 1fr;
    border: 1px solid var(--rule);
  }

  header {
    display: grid;
    grid-template-columns: auto 1fr auto;
    gap: 1rem;
    align-items: center;
    padding: 0.5rem 1rem;
    background: var(--panel);
    border-bottom: 1px solid var(--rule);
  }

  .lead {
    display: flex;
    align-items: center;
    gap: 1rem;
    min-width: 0;
  }

  .brand {
    display: flex;
    gap: 0.625rem;
    align-items: baseline;
    font-weight: 600;
    letter-spacing: 0.02em;
  }

  .mark {
    color: var(--accent);
  }

  .venue {
    font-weight: 400;
    font-size: var(--text-xs);
    white-space: nowrap;
  }

  .figures {
    justify-self: center;
    display: flex;
    flex-wrap: wrap;
    justify-content: center;
    align-items: baseline;
    gap: 0 1.5rem;
    min-width: 0;
  }

  .day {
    display: flex;
    gap: 0.625rem;
    align-items: baseline;
    font-size: var(--text-lg);
    white-space: nowrap;
  }

  .day .dim {
    font-size: var(--text-xs);
    text-transform: uppercase;
    letter-spacing: 0.12em;
  }

  .equity {
    display: flex;
    gap: 0.625rem;
    align-items: baseline;
    font-size: var(--text-2xl);
  }

  .equity .dim {
    font-size: var(--text-xs);
    text-transform: uppercase;
    letter-spacing: 0.12em;
  }

  /* The top bar gets the window, so it alone answers to the window's width. */
  @media (max-width: 64rem) {
    .venue,
    .equity .dim,
    .day .dim {
      display: none;
    }
  }

  .right {
    justify-self: end;
    display: flex;
    align-items: center;
    gap: 0.75rem;
    min-width: 0;
  }

  /* The trader's switch, when it is not "active": the one thing on the bar
     that says orders will be refused. */
  .trading {
    padding: 0.1875rem 0.625rem;
    font-size: var(--text-xs);
    font-weight: 600;
    letter-spacing: 0.04em;
    white-space: nowrap;
    border: 1px solid currentColor;
    border-radius: 999rem;
  }

  .trading.halted {
    color: var(--down);
    background: color-mix(in srgb, var(--down) 14%, transparent);
  }

  .trading.reducing {
    color: var(--accent);
    background: color-mix(in srgb, var(--accent) 14%, transparent);
  }

  .status {
    display: flex;
    gap: 0.5rem;
    align-items: center;
    font-size: var(--text-xs);
    text-transform: uppercase;
    letter-spacing: 0.12em;
    color: var(--text-dim);
  }

  .dot {
    width: 0.5rem;
    height: 0.5rem;
    border-radius: 50%;
    background: var(--text-dim);
  }

  .status.up {
    --tone: var(--up);
  }

  .status.degraded,
  .status.unknown,
  .status.reconnecting {
    --tone: var(--accent);
  }

  .status.down {
    --tone: var(--down);
  }

  .status:not(.connecting) {
    color: var(--tone);
  }

  .status:not(.connecting) .dot {
    background: var(--tone);
    box-shadow: 0 0 0 3px color-mix(in srgb, var(--tone) 25%, transparent);
  }

  .status.reconnecting .dot {
    animation: pulse 1.2s ease-in-out infinite;
  }

  @keyframes pulse {
    50% {
      opacity: 0.35;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .status.reconnecting .dot {
      animation: none;
    }
  }

  main {
    min-height: 0;
  }
</style>
