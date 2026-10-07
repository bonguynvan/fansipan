import { describe, expect, it } from "vitest";
import { UNTYPED, isSendable, typedOr } from "./ticket.js";

describe("ticket fields", () => {
  it("show the default until something is typed, and stay empty once cleared", () => {
    expect(typedOr(UNTYPED, 100)).toBe(100);
    expect(typedOr(15000, 100)).toBe(15000);
    expect(typedOr(null, 100)).toBeNull();
  });

  it("send only a number above zero", () => {
    expect(isSendable(100)).toBe(true);
    expect(isSendable("62500")).toBe(true);
    expect(isSendable(null)).toBe(false);
    expect(isSendable(UNTYPED)).toBe(false);
    expect(isSendable("")).toBe(false);
    expect(isSendable(0)).toBe(false);
  });
});
