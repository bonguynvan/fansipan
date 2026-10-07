import { describe, expect, it } from "vitest";
import { amendProblem, amendTerm, canAmendOrder } from "./amend.js";

const order = { id: 7, quantity: "300", filled_quantity: "100", limit_price: "25.5", amendable: true };

describe("canAmendOrder", () => {
  it("offers an amendable order on a venue that amends", () => {
    expect(canAmendOrder(order, true)).toBe(true);
  });

  it("offers nothing on a venue that does not amend", () => {
    expect(canAmendOrder(order, false)).toBe(false);
  });

  it("offers nothing for an order the engine would not amend", () => {
    expect(canAmendOrder({ ...order, amendable: false }, true)).toBe(false);
    expect(canAmendOrder(undefined, true)).toBe(false);
  });
});

describe("amendProblem", () => {
  it("lets a new price or a new quantity through", () => {
    expect(amendProblem(order, 300, 25.6)).toBeNull();
    expect(amendProblem(order, "400", "25.5")).toBeNull();
  });

  it("holds back the order's own terms", () => {
    expect(amendProblem(order, 300, 25.5)).toBe("unchanged");
    expect(amendProblem(order, "300.0", "25.50")).toBe("unchanged");
  });

  it("holds back a quantity not above what has filled", () => {
    expect(amendProblem(order, 100, 25.5)).toBe("belowFilled");
    expect(amendProblem(order, 50, 26)).toBe("belowFilled");
  });

  it("holds back an empty or non-positive field", () => {
    expect(amendProblem(order, null, 25.5)).toBe("quantity");
    expect(amendProblem(order, 0, 25.5)).toBe("quantity");
    expect(amendProblem(order, 300, "")).toBe("price");
    expect(amendProblem(order, 300, -1)).toBe("price");
  });
});

describe("amendTerm", () => {
  it("sends an untouched term as the engine gave it", () => {
    expect(amendTerm(25.5, "25.50")).toBe("25.50");
    expect(amendTerm(300, "300")).toBe("300");
  });

  it("sends a changed term as typed", () => {
    expect(amendTerm(25.6, "25.50")).toBe("25.6");
  });

  it("sends an empty field as empty, for Rust to refuse", () => {
    expect(amendTerm(null, "300")).toBe("");
    expect(amendTerm(undefined, "300")).toBe("");
  });
});
