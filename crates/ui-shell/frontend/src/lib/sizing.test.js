import { describe, expect, it } from "vitest";
import english from "../i18n/locales/en.json";
import { keptBudget, plain, sizingArgs, sizingMessage } from "./sizing.js";

const limit = { kind: "limit", time_in_force: "day" };
const ticket = (patch = {}) => ({
  symbol: "AAPL",
  side: "buy",
  orderType: limit,
  entry: 150,
  stop: 145,
  risk: 500,
  percent: false,
  ...patch,
});

describe("sizing from risk", () => {
  it("asks only with an entry, a stop, and a budget, all as text", () => {
    expect(sizingArgs(ticket())).toEqual({
      symbol: "AAPL",
      side: "buy",
      kind: "limit",
      timeInForce: "day",
      entry: "150",
      stop: "145",
      risk: "500",
      percent: false,
    });
    for (const missing of [{ symbol: null }, { orderType: null }, { entry: null }, { stop: "" }, { risk: 0 }]) {
      expect(sizingArgs(ticket(missing))).toBeNull();
    }
  });

  it("reads the answer as sized, capped, or too small, in words every case has", () => {
    expect(sizingMessage({ quantity: "100", capped_by: null })).toBe("ticket.risk.sized");
    expect(sizingMessage({ quantity: "20", capped_by: "buying_power" })).toBe("ticket.risk.capped");
    expect(sizingMessage({ quantity: "0", capped_by: null })).toBe("ticket.risk.none");
    expect(sizingMessage({ quantity: "0", capped_by: "holding" })).toBe("ticket.risk.cappedNone");
    for (const id of ["ticket.risk.sized", "ticket.risk.capped", "ticket.risk.cappedNone", "ticket.risk.none"]) {
      expect(english[id]).toBeTruthy();
    }
    for (const bound of ["buying_power", "holding", "order_size", "order_value"]) {
      expect(english[`ticket.risk.bound.${bound}`]).toBeTruthy();
    }
  });

  it("sends prices as plain decimals, never in exponent form", () => {
    expect(plain(0.0000001)).toBe("0.0000001");
    expect(plain(12.5)).toBe("12.5");
    expect(sizingArgs(ticket({ entry: 0.0000002, stop: 0.0000001 }))).toMatchObject({ entry: "0.0000002", stop: "0.0000001" });
  });

  it("keeps a budget only as a positive number and a unit", () => {
    expect(keptBudget({ risk: "1", percent: true })).toEqual({ risk: "1", percent: true });
    expect(keptBudget({ risk: -3, percent: "yes" })).toEqual({ risk: "", percent: false });
    expect(keptBudget(null)).toEqual({ risk: "", percent: false });
  });
});
