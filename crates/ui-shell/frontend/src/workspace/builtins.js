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
    defaultParams: { link: "a", list: "all" },
    description: "screen.watchlist.description",
  });
  registerScreen({
    id: "chart",
    title: "screen.chart.title",
    component: Chart,
    linkable: true,
    defaultParams: { link: "a" },
    description: "screen.chart.description",
  });
  registerScreen({
    id: "order-ticket",
    title: "screen.order-ticket.title",
    component: OrderTicket,
    linkable: true,
    defaultParams: { link: "a" },
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
    defaultParams: { link: "a" },
    description: "screen.account.description",
  });

  registerScreen({
    id: "price-board",
    title: "screen.price-board.title",
    component: PriceBoard,
    linkable: true,
    defaultParams: { link: "a", exchange: "" },
    description: "screen.price-board.description",
  });

  registerScreen({
    id: "time-and-sales",
    title: "screen.time-and-sales.title",
    component: TimeAndSales,
    linkable: true,
    defaultParams: { link: "a" },
    description: "screen.time-and-sales.description",
  });

  registerScreen({
    id: "depth-ladder",
    title: "screen.depth-ladder.title",
    component: DepthLadder,
    linkable: true,
    defaultParams: { link: "a" },
    description: "screen.depth-ladder.description",
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
    build(add) {
      const watchA = add("watchlist", { initialWidth: 380, title: "layout.two-charts.watchlist-a" });
      const chartA = add("chart", { position: { referencePanel: watchA, direction: "right" }, title: "layout.two-charts.chart-a" });
      add("order-ticket", { position: { referencePanel: chartA, direction: "right" }, initialWidth: 320 });
      const watchB = add("watchlist", {
        params: { link: "b" },
        title: "layout.two-charts.watchlist-b",
        position: { referencePanel: watchA, direction: "below" },
      });
      add("chart", { params: { link: "b" }, title: "layout.two-charts.chart-b", position: { referencePanel: chartA, direction: "below" } });
      add("orders", { position: { referencePanel: watchB, direction: "within" } });
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
  // exchange. Each exchange's session comes from the adapter (ADR 0009).
  registerMarketPreset({
    id: "vn",
    priceScale: 1000,
    priceDecimals: 2,
    layout: "board",
  });
}
