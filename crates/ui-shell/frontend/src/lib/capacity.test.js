import { describe, expect, it } from "vitest";
import { capacityArgs } from "./capacity.js";

const LIMIT = { kind: "limit", time_in_force: "day" };
const MARKET = { kind: "market", time_in_force: "day" };
const ticket = (overrides) => ({
  symbol: "VNM",
  side: "buy",
  orderType: LIMIT,
  priced: true,
  limitPrice: null,
  ...overrides,
});

describe("capacityArgs", () => {
  it("asks at the typed limit", () => {
    expect(capacityArgs(ticket({ limitPrice: 61000 }))).toEqual({
      symbol: "VNM",
      side: "buy",
      limitPrice: "61000",
      kind: "limit",
      timeInForce: "day",
    });
  });

  it("leaves the price to core until a limit is typed", () => {
    expect(capacityArgs(ticket({ limitPrice: "" })).limitPrice).toBeNull();
    expect(capacityArgs(ticket({ limitPrice: null })).limitPrice).toBeNull();
    expect(capacityArgs(ticket({ limitPrice: 0 })).limitPrice).toBeNull();
  });

  it("sends no price for an unpriced order, which core values at the ceiling", () => {
    expect(capacityArgs(ticket({ orderType: MARKET, priced: false, limitPrice: 61000 })).limitPrice).toBeNull();
  });

  it("asks nothing without a symbol or a type the market takes", () => {
    expect(capacityArgs(ticket({ symbol: null }))).toBeNull();
    expect(capacityArgs(ticket({ orderType: null }))).toBeNull();
  });
});
