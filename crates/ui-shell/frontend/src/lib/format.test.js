import { describe, expect, it } from "vitest";
import { currencyDigits, money, price, timeOfDay } from "./format.js";

describe("formatting", () => {
  it("writes a time of day to the second, 24-hour, in local time", () => {
    expect(timeOfDay(new Date(2026, 9, 7, 9, 5, 7).getTime())).toBe("09:05:07");
    expect(timeOfDay(new Date(2026, 9, 7, 14, 30, 0).getTime())).toBe("14:30:00");
    expect(timeOfDay(null)).toBe("—");
  });

  it("writes money in the currency's minor units", () => {
    expect(currencyDigits("VND")).toBe(0);
    expect(currencyDigits("USD")).toBe(2);
    expect(money("2000000000", "VND")).toBe("2,000,000,000");
    expect(money("100000", "USD")).toBe("100,000.00");
  });

  it("falls back to two places for a missing or unknown currency", () => {
    expect(currencyDigits(undefined)).toBe(2);
    expect(currencyDigits("not a currency")).toBe(2);
  });

  it("writes prices at the instrument's precision when given one", () => {
    expect(price("62500", 0)).toBe("62,500");
    expect(price("190.1", 2)).toBe("190.10");
    expect(price("190.125")).toBe("190.125");
    expect(price(null, 2)).toBe("—");
  });
});
