<script>
  import { onMount } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import { market, startMarket } from "./lib/market.svelte.js";
  import { flushLinks, startLinks } from "./lib/links.svelte.js";
  import { mark, money } from "./lib/format.js";
  import { registerBuiltins } from "./workspace/builtins.js";
  import Workspace from "./workspace/Workspace.svelte";
  import WorkspaceMenu from "./workspace/WorkspaceMenu.svelte";
  import { MAIN_WINDOW, currentWindowLabel, flushLayout } from "./workspace/workspace.svelte.js";

  // Screens must be registered before any layout that names them loads.
  registerBuiltins();

  const label = currentWindowLabel();

  onMount(() => {
    startLinks();
    startMarket();
    // The shell sends this just before quitting and waits briefly, so saves
    // still inside their debounce window are not lost.
    const flushing = listen("workspace-flush", () => {
      flushLayout();
      flushLinks();
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
</script>

<div class="terminal">
  <header>
    <div class="brand">
      <span class="mark" aria-hidden="true">▲</span>
      <span>Fansipan</span>
      <span class="venue dim">sim · simulated venue{label === MAIN_WINDOW ? "" : " · extra window"}</span>
    </div>
    <div class="equity">
      <span class="dim">equity</span>
      <span class="num">{money(equity)}</span>
    </div>
    <div class="tools">
      <WorkspaceMenu />
      <div class="status" class:live={market.ready}>
        <span class="dot"></span>{market.ready ? "live" : "connecting"}
      </div>
    </div>
  </header>

  <main>
    <Workspace {label} />
  </main>
</div>

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

  .status.live {
    color: var(--up);
  }

  .status.live .dot {
    background: var(--up);
    box-shadow: 0 0 0 3px color-mix(in srgb, var(--up) 25%, transparent);
  }

  main {
    min-height: 0;
  }
</style>
