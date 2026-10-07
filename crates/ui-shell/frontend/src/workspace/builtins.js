// The screens and layouts Fansipan ships with, registered through the same
// calls a third-party screen uses. Screen ids are saved in layouts: never
// rename one that has shipped.

import { registerLayout, registerScreen } from "./registry.js";
import Watchlist from "../lib/Watchlist.svelte";
import Chart from "../lib/Chart.svelte";
import OrderTicket from "../lib/OrderTicket.svelte";
import Blotter from "../lib/Blotter.svelte";
import Portfolio from "../lib/Portfolio.svelte";

export const DEFAULT_LAYOUT = "trading";

let registered = false;

export function registerBuiltins() {
  if (registered) return;
  registered = true;

  registerScreen({
    id: "watchlist",
    title: "Watchlist",
    component: Watchlist,
    linkable: true,
    defaultParams: { link: "a" },
    description: "Live bid/ask with two-minute sparklines; picks the symbol for its link group.",
  });
  registerScreen({
    id: "chart",
    title: "Chart",
    component: Chart,
    linkable: true,
    defaultParams: { link: "a" },
    description: "Candles built by the engines from live quotes.",
  });
  registerScreen({
    id: "order-ticket",
    title: "Order ticket",
    component: OrderTicket,
    linkable: true,
    defaultParams: { link: "a" },
    description: "Buy or sell the linked symbol; risk checks run before the venue sees it.",
  });
  registerScreen({
    id: "orders",
    title: "Orders",
    component: Blotter,
    description: "Every order this session, with fills, status, reason, and cancel.",
  });
  registerScreen({
    id: "account",
    title: "Account",
    component: Portfolio,
    linkable: true,
    defaultParams: { link: "a" },
    description: "Cash and positions as the venue reports them, with live P&L.",
  });

  registerLayout({
    id: DEFAULT_LAYOUT,
    title: "Trading desk",
    // Orders is placed before the chart so it spans the chart and ticket.
    build(add) {
      const watchlist = add("watchlist", { initialWidth: 420 });
      const orders = add("orders", { position: { referencePanel: watchlist, direction: "right" }, initialHeight: 270 });
      const chart = add("chart", { position: { referencePanel: orders, direction: "above" } });
      add("order-ticket", { position: { referencePanel: chart, direction: "right" }, initialWidth: 340 });
      add("account", { position: { referencePanel: watchlist, direction: "below" }, initialHeight: 270 });
    },
  });

  registerLayout({
    id: "two-charts",
    title: "Two charts (link groups A and B)",
    build(add) {
      const watchA = add("watchlist", { initialWidth: 380, title: "Watchlist A" });
      const chartA = add("chart", { position: { referencePanel: watchA, direction: "right" }, title: "Chart A" });
      add("order-ticket", { position: { referencePanel: chartA, direction: "right" }, initialWidth: 320 });
      const watchB = add("watchlist", {
        params: { link: "b" },
        title: "Watchlist B",
        position: { referencePanel: watchA, direction: "below" },
      });
      add("chart", { params: { link: "b" }, title: "Chart B", position: { referencePanel: chartA, direction: "below" } });
      add("orders", { position: { referencePanel: watchB, direction: "within" } });
    },
  });
}
