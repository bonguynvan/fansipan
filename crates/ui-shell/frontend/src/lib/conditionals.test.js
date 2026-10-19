import { describe, expect, it } from "vitest";
import english from "../i18n/locales/en.json";
import { TRIGGERS, complete, conditionWords, inOrder, isLive, placeArgs, withConditionals } from "./conditionals.js";

const form = (patch = {}) => ({
  symbol: "FPT",
  side: "sell",
  quantity: 100,
  trigger: "at_or_below",
  price: 110000,
  offset: "",
  percent: false,
  sendAs: "marketable",
  limitPrice: "",
  ...patch,
});

describe("a conditional form", () => {
  it("is complete with what its trigger and its order need", () => {
    expect(complete(form())).toBe(true);
    expect(complete(form({ price: "" }))).toBe(false);
    expect(complete(form({ trigger: "trailing" }))).toBe(false);
    expect(complete(form({ trigger: "trailing", offset: 3 }))).toBe(true);
    expect(complete(form({ sendAs: "limit" }))).toBe(false);
    expect(complete(form({ sendAs: "limit", limitPrice: 109000 }))).toBe(true);
    expect(complete(form({ symbol: null }))).toBe(false);
  });

  it("sends only the fields its trigger uses, as text", () => {
    expect(placeArgs(form())).toEqual({
      symbol: "FPT",
      side: "sell",
      quantity: "100",
      trigger: "at_or_below",
      price: "110000",
      offset: null,
      percent: false,
      limitPrice: null,
    });
    expect(placeArgs(form({ trigger: "trailing", offset: 3, percent: true, sendAs: "limit", limitPrice: 1 }))).toMatchObject({
      price: null,
      offset: "3",
      percent: true,
      limitPrice: "1",
    });
  });
});

describe("a held conditional", () => {
  it("reads as its condition, in words every trigger has", () => {
    expect(conditionWords({ trigger: "at_or_above", price: "12" })).toEqual({
      id: "conditional.when.at_or_above",
      vars: { price: "12" },
    });
    expect(conditionWords({ trigger: "trailing", offset: "3", percent: true }).id).toBe("conditional.when.trailingPercent");
    for (const trigger of TRIGGERS) {
      expect(english[`conditional.trigger.${trigger}`]).toBeTruthy();
      expect(english[conditionWords({ trigger, percent: false }).id]).toBeTruthy();
    }
    for (const status of ["pending", "waiting", "fired", "cancelled"]) expect(english[`conditional.status.${status}`]).toBeTruthy();
  });

  it("lists live ones first, an exit pending on its entry among them, newest first", () => {
    const listed = inOrder([
      { id: 1, status: "waiting" },
      { id: 2, status: "fired" },
      { id: 3, status: "waiting" },
      { id: 4, status: "pending" },
    ]);

    expect(listed.map((c) => c.id)).toEqual([4, 3, 1, 2]);
    expect(isLive({ status: "pending" })).toBe(true);
    expect(isLive({ status: "cancelled" })).toBe(false);
  });

  it("takes a batch's changes over what is held", () => {
    const held = { 1: { id: 1, status: "waiting" } };

    expect(withConditionals(held, [{ id: 1, status: "fired" }, { id: 2, status: "waiting" }])).toEqual({
      1: { id: 1, status: "fired" },
      2: { id: 2, status: "waiting" },
    });
    expect(withConditionals(held, [])).toBe(held);
  });
});
