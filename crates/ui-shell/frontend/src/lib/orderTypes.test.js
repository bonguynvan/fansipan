import { describe, expect, it } from "vitest";
import { orderTypeName, phaseName } from "./orderTypes.js";

/** Says each id back, so a test reads which message was chosen. */
const t = (id) => `<${id}>`;

describe("order types and phases in words", () => {
  it("use the venue's own name when it has one", () => {
    expect(orderTypeName({ kind: "market", time_in_force: "atOpen", name: "ATO" }, t)).toBe("ATO");
    expect(phaseName({ phase: "openingAuction", name: "ATO" }, t)).toBe("ATO");
  });

  it("say a generic type by its kind, and its time in force unless it is for the day", () => {
    expect(orderTypeName({ kind: "limit", time_in_force: "day", name: null }, t)).toBe("<orderKind.limit>");
    expect(orderTypeName({ kind: "market", time_in_force: "ioc" }, t)).toBe(
      "<orderKind.market> · <timeInForce.ioc>",
    );
  });

  it("call what it has no word for unknown", () => {
    expect(orderTypeName({ kind: "stop", time_in_force: "gtc" }, t)).toBe(
      "<orderKind.unknown> · <timeInForce.unknown>",
    );
    expect(phaseName({ phase: "halt", name: null }, t)).toBe("<session.unknown>");
  });
});
