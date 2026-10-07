import { describe, expect, it } from "vitest";
import { pickToApply, received } from "./picks.svelte.js";

const pick = { id: "p1", symbol: "VNM", price: "62400", side: "buy" };

describe("pickToApply", () => {
  it("applies a new pick for the ticket's group and symbol", () => {
    expect(pickToApply({ a: pick }, "a", "VNM", null)).toBe(pick);
  });

  it("applies a pick once", () => {
    expect(pickToApply({ a: pick }, "a", "VNM", "p1")).toBeNull();
  });

  it("ignores another group's pick, another symbol's, and an unlinked ticket", () => {
    expect(pickToApply({ b: pick }, "a", "VNM", null)).toBeNull();
    expect(pickToApply({ a: pick }, "a", "FPT", null)).toBeNull();
    expect(pickToApply({ a: pick }, "none", "VNM", null)).toBeNull();
  });
});

describe("received", () => {
  it("takes a pick from another window", () => {
    expect(received({}, { group: "a", pick }, new Set())).toEqual({ a: pick });
  });

  it("drops the echo of a pick already taken, so an older pick cannot come back", () => {
    const newer = { ...pick, id: "p2", price: "62500" };

    const latest = received({ a: newer }, { group: "a", pick }, new Set(["p1", "p2"]));

    expect(latest).toEqual({ a: newer });
  });

  it("drops a payload for no group or without a pick", () => {
    const latest = {};

    expect(received(latest, { group: "none", pick }, new Set())).toBe(latest);
    expect(received(latest, { group: "a" }, new Set())).toBe(latest);
  });
});
