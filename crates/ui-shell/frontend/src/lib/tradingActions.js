// The keyboard actions Fansipan ships with, registered the way a plugin
// registers its own. Each returns a notice for the trader, or nothing.
// Defaults follow common broker practice in Vietnam: F1 buys, F2 sells.
// Cancelling every order takes a second press, since a stray one would
// cost every order its place in the queue.

import { invoke } from "@tauri-apps/api/core";
import { registerAction } from "./hotkeys.svelte.js";
import { activeTicket } from "./tickets.svelte.js";

const NO_TICKET = { id: "hotkey.noTicket" };

/** Readies the ticket in use: `side` set (null leaves it), quantity focused. */
function onTicket(side) {
  return () => {
    const ticket = activeTicket();
    if (!ticket) return NO_TICKET;
    ticket.prepare(side);
    return null;
  };
}

/** The trader's switch, as the shell names its states. */
export const TRADING_STATES = ["active", "reducing", "halted"];

/**
 * Sets the switch for every window: halted refuses new orders and
 * amendments, reducing takes only orders that close a position. Working
 * orders stay working; cancels are never stopped.
 */
export async function setTradingState(state) {
  await invoke("set_trading_state", { state });
  return { id: `trading.notice.${state}` };
}

/** How long a first press of cancel-all waits for the second. */
export const CONFIRM_MS = 2000;

/**
 * The notice for a cancel-all result: what was sent, or the first
 * refusal. With `symbol`, the notice names it, so the trader knows which
 * orders went.
 */
export function cancelNotice({ sent, failed }, symbol = null) {
  const vars = symbol ? { symbol } : {};
  const scope = symbol ? "Symbol" : "";
  if (failed.length) {
    return {
      id: "hotkey.cancelFailed",
      count: failed.length,
      vars: { ...vars, sent: sent.length, error: failed[0].error },
    };
  }
  return sent.length
    ? { id: `hotkey.cancelSent${scope}`, count: sent.length, vars }
    : { id: `hotkey.nothingToCancel${scope}`, vars };
}

/** Cancels every working order, or every one in `symbol`; returns the notice for the result. */
export async function cancelAllOrders(symbol = null) {
  return cancelNotice(await invoke("cancel_all_orders", { symbol }), symbol);
}

/**
 * Runs `act` on the second call within CONFIRM_MS of the first; the first
 * only arms it and says so. `now` is injectable for tests.
 */
export function twoPress(act, armedNotice, now = () => Date.now()) {
  let armedAt = null;
  return () => {
    const at = now();
    if (armedAt !== null && at - armedAt <= CONFIRM_MS) {
      armedAt = null;
      return act();
    }
    armedAt = at;
    return armedNotice;
  };
}

export function registerTradingActions() {
  registerAction({ id: "ticket.buy", title: "hotkey.action.buy", defaultKey: "F1", run: onTicket("buy") });
  registerAction({ id: "ticket.sell", title: "hotkey.action.sell", defaultKey: "F2", run: onTicket("sell") });
  registerAction({ id: "ticket.focus", title: "hotkey.action.focus", defaultKey: "F3", run: onTicket(null) });
  registerAction({
    id: "orders.cancel-symbol",
    title: "hotkey.action.cancelSymbol",
    defaultKey: "F9",
    run: () => {
      const symbol = activeTicket()?.symbol();
      return symbol ? cancelAllOrders(symbol) : NO_TICKET;
    },
  });
  // A halt cancels nothing, so it needs no second press.
  registerAction({
    id: "trading.halt",
    title: "hotkey.action.halt",
    defaultKey: "Ctrl+Shift+H",
    run: () => setTradingState("halted"),
  });
  registerAction({
    id: "orders.cancel-all",
    title: "hotkey.action.cancelAll",
    defaultKey: "Shift+F9",
    run: twoPress(() => cancelAllOrders(null), { id: "hotkey.confirmCancelAll" }),
  });
}
