import { describe, expect, it } from "vitest";
import { LADDER_SPAN, MAX_LADDER_ROWS, centreIndex, ladderPrices, ladderRows, priceKey } from "./ladder.js";
import { tickTable } from "./ticks.js";

const hose = tickTable({
  ticks: [
    { from: "0", tick: "10" },
    { from: "10000", tick: "50" },
    { from: "50000", tick: "100" },
  ],
});
const cents = tickTable({ ticks: [{ from: "0", tick: "0.01" }] });

const level = (price, size) => ({ price, size });
const vnm = {
  reference: "62500",
  ceiling: "62800",
  floor: "62200",
  bids: [level("62400", "1000"), level("62300", "2000")],
  asks: [level("62500", "1500")],
  last: "62500",
};

describe("ladderPrices", () => {
  it("runs floor to ceiling, highest first, on the tick", () => {
    expect(ladderPrices(hose, vnm)).toEqual([62800, 62700, 62600, 62500, 62400, 62300, 62200]);
  });

  it("follows the tick across a step of the table", () => {
    const board = { floor: "9970", ceiling: "10100" };

    expect(ladderPrices(hose, board)).toEqual([10100, 10050, 10000, 9990, 9980, 9970]);
  });

  it("without a band, spans the market a fixed number of ticks each way", () => {
    const board = { bids: [level("190.01", "100")], asks: [level("190.02", "100")] };

    const prices = ladderPrices(cents, board);

    expect(prices).toHaveLength(2 * LADDER_SPAN + 2);
    expect(prices).toContain(190.01);
    expect(prices).toContain(190.02);
    expect(prices[0]).toBeGreaterThan(prices.at(-1));
  });

  it("shows the prices around the market when a band holds more than it can", () => {
    const board = { floor: "1.00", ceiling: "100.00", bids: [level("50.00", "100")], asks: [level("50.01", "100")] };

    const prices = ladderPrices(cents, board);

    expect(prices.length).toBeLessThanOrEqual(MAX_LADDER_ROWS);
    expect(Math.abs(prices.indexOf(50) - prices.length / 2)).toBeLessThan(3);
    expect(prices).toContain(50.01);
  });

  it("is empty when nothing places it", () => {
    expect(ladderPrices(cents, {})).toEqual([]);
    expect(ladderPrices(cents, undefined)).toEqual([]);
  });
});

describe("ladderRows", () => {
  it("puts each book level's size at its price", () => {
    const rows = ladderRows(hose, vnm);
    const at = (key) => rows.find((row) => row.key === key);

    expect([at("62400").bid, at("62300").bid, at("62500").ask]).toEqual([1000, 2000, 1500]);
    expect([at("62600").bid, at("62600").ask]).toEqual([null, null]);
  });

  it("marks the touch and the last match", () => {
    const rows = ladderRows(hose, vnm);

    expect(rows.filter((row) => row.touch).map((row) => row.key)).toEqual(["62500", "62400"]);
    expect(rows.filter((row) => row.last).map((row) => row.key)).toEqual(["62500"]);
  });

  it("scales prices for display but keeps the exact price to send", () => {
    const row = ladderRows(hose, vnm, 1000).find((r) => r.key === "62700");

    expect(row.price).toBe(62.7);
  });

  it("tones prices against reference, ceiling and floor", () => {
    const rows = ladderRows(hose, vnm);
    const tone = (key) => rows.find((row) => row.key === key).tone;

    expect([tone("62800"), tone("62200"), tone("62500"), tone("62600"), tone("62400")]).toEqual([
      "ceiling",
      "floor",
      "ref",
      "up",
      "down",
    ]);
  });

  it("matches cent prices whatever their written form", () => {
    const board = { bids: [level("190.1", "300")], asks: [level("190.11", "100")] };

    const row = ladderRows(cents, board).find((r) => r.key === priceKey(190.1, 2));

    expect(row.bid).toBe(300);
  });
});

describe("centreIndex", () => {
  it("centres on the best bid", () => {
    const rows = ladderRows(hose, vnm);

    expect(rows[centreIndex(rows)].key).toBe("62400");
  });

  it("falls back to the last match, then the middle row", () => {
    const lastOnly = ladderRows(hose, { ...vnm, bids: [], asks: [] });
    const nothing = ladderRows(hose, { ...vnm, bids: [], asks: [], last: null });

    expect(lastOnly[centreIndex(lastOnly)].key).toBe("62500");
    expect(centreIndex(nothing)).toBe(Math.floor(nothing.length / 2));
  });
});
