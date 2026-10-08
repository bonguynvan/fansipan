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
  import { mark, money } from "./lib/format.js";
  import { registerBuiltins } from "./workspace/builtins.js";
  import Workspace from "./workspace/Workspace.svelte";
  import WorkspaceMenu from "./workspace/WorkspaceMenu.svelte";
  import { MAIN_WINDOW, currentWindowLabel, flushLayout, workspace } from "./workspace/workspace.svelte.js";
  import { flushLocale, startLocale, t } from "./i18n/i18n.svelte.js";
  import LocalePicker from "./i18n/LocalePicker.svelte";

  // Screens must be registered before any layout that names them loads.
  registerBuiltins();

  const label = currentWindowLabel();
  let keysOpen = $state(false);

  onMount(() => {
    const failed = (err) => (workspace.notice = { id: "app.startFailed", vars: { error: String(err) } });
    startLocale().catch(failed);
    startLinks({
      onError: (error) => (workspace.notice = { id: "workspace.symbolNotShared", vars: { error } }),
    }).catch(failed);
    startPicks().catch(failed);
    startHotkeys({ onNotice: (notice) => (workspace.notice = notice) }).catch(failed);
    startMarket().catch(failed);
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

<div class="terminal">
  <header>
    <div class="brand">
      <span class="mark" aria-hidden="true">▲</span>
      <span>Fansipan</span>
      <span class="venue dim"
        >{market.info?.title ?? ""}{label === MAIN_WINDOW ? "" : ` · ${t("app.extraWindow")}`}</span
      >
    </div>
    <div class="equity">
      <span class="dim">{t("app.equity")}</span>
      <span class="num">{money(equity, market.info?.currency)}</span>
    </div>
    <div class="tools">
      <WorkspaceMenu />
      <button class="keys-button" onclick={() => (keysOpen = true)}>{t("hotkey.open")}</button>
      <LocalePicker />
      <div class="status {market.ready ? connection.state : 'connecting'}" title={statusDetail} role="status">
        <span class="dot"></span>{status}
      </div>
    </div>
  </header>

  <main>
    <Workspace {label} />
  </main>
</div>

<KeysDialog bind:open={keysOpen} />

<style>
  .terminal {
    display: grid;
    grid-template-rows: auto 1fr;
    height: 100vh;
  }

  header {
    display: grid;
    grid-template-columns: 1fr auto 1fr;
    align-items: center;
    padding: 8px 16px;
    background: var(--panel);
    border-bottom: 1px solid var(--rule);
  }

  .brand {
    display: flex;
    gap: 10px;
    align-items: baseline;
    font-weight: 600;
    letter-spacing: 0.02em;
  }

  .mark {
    color: var(--accent);
  }

  .venue {
    font-weight: 400;
    font-size: 11px;
  }

  .equity {
    display: flex;
    gap: 10px;
    align-items: baseline;
    font-size: 18px;
  }

  .equity .dim {
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.12em;
  }

  .keys-button {
    padding: 4px 10px;
    font: inherit;
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    background: transparent;
    border: 1px solid var(--rule);
    color: var(--text-dim);
    cursor: pointer;
  }

  .keys-button:hover,
  .keys-button:focus-visible {
    color: var(--text);
    border-color: var(--text-dim);
    outline: none;
  }

  .tools {
    justify-self: end;
    display: flex;
    gap: 16px;
    align-items: center;
  }

  .status {
    display: flex;
    gap: 8px;
    align-items: center;
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.12em;
    color: var(--text-dim);
  }

  .dot {
    width: 8px;
    height: 8px;
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
