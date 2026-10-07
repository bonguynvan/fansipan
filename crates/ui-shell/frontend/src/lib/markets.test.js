import { describe, expect, it } from "vitest";
import { marketPreset, registerMarketPreset } from "./markets.js";

describe("market presets", () => {
  it("fill unset fields from the fallback and are frozen", () => {
    registerMarketPreset({ id: "test-thousands", priceScale: 1000, layout: "board" });

    const preset = marketPreset("test-thousands");
    expect(preset).toMatchObject({ priceScale: 1000, layout: "board", schedule: null, timeZone: null });
    expect(Object.isFrozen(preset)).toBe(true);
  });

  it("give an unknown or missing market the plain fallback", () => {
    expect(marketPreset("nowhere")).toMatchObject({ priceScale: 1, schedule: null, layout: null });
    expect(marketPreset(undefined).priceScale).toBe(1);
  });

  it("refuse a bad id, a duplicate, a non-positive scale and a malformed schedule", () => {
    registerMarketPreset({ id: "test-once" });

    expect(() => registerMarketPreset({ id: "Bad Id" })).toThrow(/must be/);
    expect(() => registerMarketPreset({ id: "test-once" })).toThrow(/already registered/);
    expect(() => registerMarketPreset({ id: "test-zero", priceScale: 0 })).toThrow(/priceScale/);
    expect(() => registerMarketPreset({ id: "test-sched", schedule: "9-15" })).toThrow(/schedule/);
    expect(() => registerMarketPreset({ id: "test-places", priceDecimals: 1.5 })).toThrow(/priceDecimals/);
  });
});
