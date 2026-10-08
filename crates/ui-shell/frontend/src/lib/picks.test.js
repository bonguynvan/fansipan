import { describe, expect, it } from "vitest";
import { pickToApply, received } from "./picks.svelte.js";

const pick = { id: "p1", symbol: "VNM", price: "62400", side: "buy" };
const linked = { linked: true };

describe("pickToApply", () => {
  it("applies a new pick for a linked ticket's symbol", () => {
    expect(pickToApply(pick, linked, "VNM", null)).toBe(pick);
  });

  it("applies a pick once", () => {
    expect(pickToApply(pick, linked, "VNM", "p1")).toBeNull();
  });

  it("ignores another symbol's pick, an unlinked ticket, and no pick", () => {
    expect(pickToApply(pick, linked, "FPT", null)).toBeNull();
    expect(pickToApply(pick, { linked: false }, "VNM", null)).toBeNull();
    expect(pickToApply(null, linked, "VNM", null)).toBeNull();
  });
});

describe("received", () => {
  it("takes a pick from another window", () => {
    expect(received(null, { pick }, new Set())).toBe(pick);
  });

  it("drops the echo of a pick already taken, so an older pick cannot come back", () => {
    const newer = { ...pick, id: "p2", price: "62500" };

    expect(received(newer, { pick }, new Set(["p1", "p2"]))).toBe(newer);
  });

  it("drops a payload without a pick", () => {
    expect(received(pick, {}, new Set())).toBe(pick);
    expect(received(pick, null, new Set())).toBe(pick);
  });
});
