import { describe, expect, it } from "vitest";
import { DEFAULT_VIEW, ordersIn, viewCounts, viewOf } from "./blotter.js";

const orders = [
  { id: 3, status: "accepted", open: true },
  { id: 2, status: "filled", open: false },
  { id: 1, status: "cancelling", open: true },
  { id: 0, status: "denied", open: false },
];
const ids = (list) => list.map((order) => order.id);

describe("blotter views", () => {
  it("opens on the working orders, and keeps a view it was given", () => {
    expect(DEFAULT_VIEW).toBe("working");
    expect(viewOf({})).toBe("working");
    expect(viewOf(undefined)).toBe("working");
    expect(viewOf({ view: "finished" })).toBe("finished");
    expect(viewOf({ view: "nonsense" })).toBe("working");
  });

  it("sorts orders by the engine's open flag, keeping their order", () => {
    expect(ids(ordersIn("working", orders))).toEqual([3, 1]);
    expect(ids(ordersIn("finished", orders))).toEqual([2, 0]);
    expect(ids(ordersIn("all", orders))).toEqual([3, 2, 1, 0]);
  });

  it("counts each view", () => {
    expect(viewCounts(orders)).toEqual({ working: 2, finished: 2, all: 4 });
    expect(viewCounts([])).toEqual({ working: 0, finished: 0, all: 0 });
  });
});
