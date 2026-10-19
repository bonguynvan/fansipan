import { describe, expect, it, vi } from "vitest";
import english from "../i18n/locales/en.json";
import { CONFIRM_MS, TRADING_STATES, cancelNotice, setTradingState, twoPress } from "./tradingActions.js";

const invoked = [];
vi.mock("@tauri-apps/api/core", () => ({
  invoke: async (command, args) => {
    invoked.push({ command, args });
    return null;
  },
}));

describe("the trading switch", () => {
  it("asks the shell for the state and says what it means", async () => {
    invoked.length = 0;

    const notice = await setTradingState("halted");

    expect(invoked).toEqual([{ command: "set_trading_state", args: { state: "halted" } }]);
    expect(notice).toEqual({ id: "trading.notice.halted" });
  });

  it("has words for every state, and a banner for each one that refuses orders", () => {
    for (const state of TRADING_STATES) {
      expect(english[`trading.state.${state}`]).toBeTruthy();
      expect(english[`trading.notice.${state}`]).toBeTruthy();
      if (state !== "active") expect(english[`trading.banner.${state}`]).toBeTruthy();
    }
  });
});

const order = (id) => ({ id, status: "cancelling" });

describe("cancelNotice", () => {
  it("counts the cancels sent", () => {
    expect(cancelNotice({ sent: [order(1), order(2)], failed: [] })).toEqual({
      id: "hotkey.cancelSent",
      count: 2,
      vars: {},
    });
  });

  it("names the symbol when one symbol's orders were cancelled", () => {
    expect(cancelNotice({ sent: [order(1)], failed: [] }, "VNM")).toEqual({
      id: "hotkey.cancelSentSymbol",
      count: 1,
      vars: { symbol: "VNM" },
    });
    expect(cancelNotice({ sent: [], failed: [] }, "VNM")).toEqual({
      id: "hotkey.nothingToCancelSymbol",
      vars: { symbol: "VNM" },
    });
  });

  it("says when there was nothing to cancel", () => {
    expect(cancelNotice({ sent: [], failed: [] })).toEqual({ id: "hotkey.nothingToCancel", vars: {} });
  });

  it("leads with a refusal, so a working order is never mistaken for cancelled", () => {
    const notice = cancelNotice({ sent: [order(1)], failed: [{ id: 2, error: "cancel not sent: socket closed" }] });

    expect(notice).toEqual({
      id: "hotkey.cancelFailed",
      count: 1,
      vars: { sent: 1, error: "cancel not sent: socket closed" },
    });
  });
});

describe("twoPress", () => {
  const armed = { id: "armed" };

  it("acts only on a second press soon after the first", () => {
    let clock = 0;
    let acted = 0;
    const press = twoPress(() => (acted += 1), armed, () => clock);

    expect(press()).toBe(armed);
    expect(acted).toBe(0);
    clock += CONFIRM_MS - 1;
    press();
    expect(acted).toBe(1);
  });

  it("starts over once the second press comes too late", () => {
    let clock = 0;
    let acted = 0;
    const press = twoPress(() => (acted += 1), armed, () => clock);

    press();
    clock += CONFIRM_MS + 1;
    expect(press()).toBe(armed);
    expect(acted).toBe(0);
    press();
    expect(acted).toBe(1);
  });
});
