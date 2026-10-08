import { describe, expect, it } from "vitest";
import { CONFIRM_MS, cancelNotice, twoPress } from "./tradingActions.js";

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
