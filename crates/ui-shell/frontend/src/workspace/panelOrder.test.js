import { describe, expect, it } from "vitest";
import { panelOrder, stepFrom } from "./panelOrder.js";

const group = (top, left, panels, floating = false) => ({ top, left, floating, panels });

describe("panelOrder", () => {
  it("reads a page top to bottom, then left to right, each group's tabs in order", () => {
    const groups = [
      group(300, 0, ["orders"]),
      group(0, 600, ["ticket"]),
      group(0, 0, ["board", "chart"]),
    ];

    expect(panelOrder(groups)).toEqual(["board", "chart", "ticket", "orders"]);
  });

  it("walks the docked panels before the floating ones", () => {
    const groups = [group(10, 10, ["floating"], true), group(500, 500, ["docked"])];

    expect(panelOrder(groups)).toEqual(["docked", "floating"]);
  });

  it("has nothing to walk on an empty page", () => {
    expect(panelOrder([])).toEqual([]);
  });
});

describe("stepFrom", () => {
  const order = ["a", "b", "c"];

  it("steps forward and back, wrapping round", () => {
    expect(stepFrom(order, "a", 1)).toBe("b");
    expect(stepFrom(order, "c", 1)).toBe("a");
    expect(stepFrom(order, "a", -1)).toBe("c");
  });

  it("starts at an end when the current item is not in the order", () => {
    expect(stepFrom(order, "z", 1)).toBe("a");
    expect(stepFrom(order, null, -1)).toBe("c");
  });

  it("has nowhere to go in an empty order", () => {
    expect(stepFrom([], "a", 1)).toBeNull();
  });
});
