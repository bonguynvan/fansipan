import { describe, expect, it } from "vitest";
import { roundLots, roundPrice, stepDown, stepUp, tickAt, tickTable } from "./ticks.js";

const hose = tickTable({
  ticks: [
    { from: "0", tick: "10" },
    { from: "10000", tick: "50" },
    { from: "50000", tick: "100" },
  ],
});
const cents = tickTable({ ticks: [{ from: "0", tick: "0.01" }] });

describe("tick ladder", () => {
  it("picks the tick of the step a price is in", () => {
    expect(tickAt(hose, 9990)).toBe(10);
    expect(tickAt(hose, 10000)).toBe(50);
    expect(tickAt(hose, 62500)).toBe(100);
  });

  it("steps across tier boundaries onto valid prices", () => {
    expect(stepDown(hose, 10000)).toBe(9990);
    expect(stepUp(hose, 9990)).toBe(10000);
    expect(stepUp(hose, 10000)).toBe(10050);
    expect(stepUp(hose, 49950)).toBe(50000);
    expect(stepDown(hose, 62500)).toBe(62400);
  });

  it("does not let float noise skip a cent", () => {
    expect(stepUp(cents, 190.01)).toBe(190.02);
    expect(stepDown(cents, 190.02)).toBe(190.01);
    expect(stepUp(cents, 0.1)).toBe(0.11);
  });

  it("rounds to the tick of where the price lands", () => {
    expect(roundPrice(hose, 10165, "down")).toBe(10150);
    expect(roundPrice(hose, 26860)).toBe(26850);
    expect(roundPrice(hose, 62520, "up")).toBe(62600);
  });

  it("falls back to a cent tick for an instrument without a ladder", () => {
    expect(tickAt(tickTable(undefined), 5)).toBe(0.01);
  });

  it("stops at the lowest tick instead of offering zero or less", () => {
    expect(stepDown(hose, 10)).toBe(10);
    expect(stepDown(hose, 0)).toBe(0);
  });

  it("steps from an off-tick price to the nearest tradable one, across a tier", () => {
    expect(stepUp(hose, 9995)).toBe(10000);
    expect(stepDown(hose, 10005)).toBe(10000);
    expect(stepUp(cents, 190.015)).toBe(190.02);
  });
});

describe("lots", () => {
  it("rounds to whole lots and never to zero", () => {
    expect(roundLots(250, 100)).toBe(300);
    expect(roundLots(20, 100)).toBe(100);
    expect(roundLots(7, 1)).toBe(7);
  });
});
