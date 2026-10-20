// The screens and layouts Fansipan ships with, registered through the same
// calls a third-party screen uses. Screen ids are saved in layouts: never
// rename one that has shipped. Titles and descriptions are message ids
// (see src/i18n/locales).

import { registerLayout, registerScreen } from "./registry.js";
import { registerMarketPreset } from "../lib/markets.js";
import { registerTradingActions } from "../lib/tradingActions.js";
import Watchlist from "../lib/Watchlist.svelte";
import Chart from "../lib/Chart.svelte";
import OrderTicket from "../lib/OrderTicket.svelte";
import Blotter from "../lib/Blotter.svelte";
import Portfolio from "../lib/Portfolio.svelte";
import PriceBoard from "../lib/PriceBoard.svelte";
import TimeAndSales from "../lib/TimeAndSales.svelte";
import DepthLadder from "../lib/DepthLadder.svelte";
import ConditionalOrders from "../lib/ConditionalOrders.svelte";

export const DEFAULT_LAYOUT = "trading";

let registered = false;

export function registerBuiltins() {
  if (registered) return;
  registered = true;

  registerScreen({
    id: "watchlist",
    title: "screen.watchlist.title",
    component: Watchlist,
    linkable: true,
    defaultParams: { linked: true, list: "all" },
    description: "screen.watchlist.description",
  });
  registerScreen({
    id: "chart",
    title: "screen.chart.title",
    component: Chart,
    linkable: true,
    defaultParams: { linked: true },
    description: "screen.chart.description",
  });
  registerScreen({
    id: "order-ticket",
    title: "screen.order-ticket.title",
    component: OrderTicket,
    linkable: true,
    defaultParams: { linked: true },
    description: "screen.order-ticket.description",
  });
  registerScreen({
    id: "orders",
    title: "screen.orders.title",
    component: Blotter,
    description: "screen.orders.description",
  });
  registerScreen({
    id: "account",
    title: "screen.account.title",
    component: Portfolio,
    linkable: true,
    defaultParams: { linked: true },
    description: "screen.account.description",
  });

  registerScreen({
    id: "price-board",
    title: "screen.price-board.title",
    component: PriceBoard,
    linkable: true,
    defaultParams: { linked: true, exchange: "" },
    description: "screen.price-board.description",
  });

  registerScreen({
    id: "time-and-sales",
    title: "screen.time-and-sales.title",
    component: TimeAndSales,
    linkable: true,
    defaultParams: { linked: true },
    description: "screen.time-and-sales.description",
  });

  registerScreen({
    id: "depth-ladder",
    title: "screen.depth-ladder.title",
    component: DepthLadder,
    linkable: true,
    defaultParams: { linked: true },
    description: "screen.depth-ladder.description",
  });

  registerScreen({
    id: "conditional-orders",
    title: "screen.conditional-orders.title",
    component: ConditionalOrders,
    linkable: true,
    defaultParams: { linked: true },
    description: "screen.conditional-orders.description",
  });

  registerTradingActions();

  registerLayout({
    id: DEFAULT_LAYOUT,
    title: "layout.trading.title",
    // Orders is placed before the chart so it spans the chart and ticket.
    build(add) {
      const watchlist = add("watchlist", { initialWidth: 420 });
      const orders = add("orders", { position: { referencePanel: watchlist, direction: "right" }, initialHeight: 270 });
      const chart = add("chart", { position: { referencePanel: orders, direction: "above" } });
      const ticket = add("order-ticket", { position: { referencePanel: chart, direction: "right" }, initialWidth: 340 });
      const tape = add("time-and-sales", { position: { referencePanel: ticket, direction: "below" } });
      add("depth-ladder", { position: { referencePanel: tape, direction: "within" } });
      add("account", { position: { referencePanel: watchlist, direction: "below" }, initialHeight: 270 });
    },
  });

  registerLayout({
    id: "two-charts",
    title: "layout.two-charts.title",
    // One chart follows the shared symbol; the other keeps its own, picked
    // from its tab strip.
    build(add) {
      const watchlist = add("watchlist", { initialWidth: 380 });
      const linked = add("chart", { position: { referencePanel: watchlist, direction: "right" }, title: "layout.two-charts.linked" });
      add("order-ticket", { position: { referencePanel: linked, direction: "right" }, initialWidth: 320 });
      add("chart", {
        params: { linked: false },
        title: "layout.two-charts.own",
        position: { referencePanel: linked, direction: "below" },
      });
      add("orders", { position: { referencePanel: watchlist, direction: "below" } });
    },
  });

  registerLayout({
    id: "board",
    title: "layout.board.title",
    build(add) {
      const board = add("price-board");
      const chart = add("chart", { position: { referencePanel: board, direction: "below" }, initialHeight: 330 });
      const ticket = add("order-ticket", { position: { referencePanel: chart, direction: "right" }, initialWidth: 300 });
      const orders = add("orders", { position: { referencePanel: ticket, direction: "right" }, initialWidth: 560 });
      add("account", { position: { referencePanel: orders, direction: "within" } });
      // Beside the board, once the bottom row spans beneath both.
      const tape = add("time-and-sales", { position: { referencePanel: board, direction: "right" }, initialWidth: 300 });
      add("depth-ladder", { position: { referencePanel: tape, direction: "within" } });
    },
  });

  // Vietnamese boards quote in thousands of dong, to two places on every
  // exchange. Each exchange's session comes from the adapter.
  registerMarketPreset({
    id: "vn",
    priceScale: 1000,
    priceDecimals: 2,
    layout: "board",
  });
}
