import { describe, expect, it } from "vitest";
import { BOARD_DEPTH, bandsOf, boardRow, boardRows, exchangesOf, toneOf } from "./board.js";

const level = (price, size) => ({ price, size });

/** VNM on HOSE: reference 62,500, band 58,200 to 66,800. */
function vnm(overrides = {}) {
  return {
    symbol: "VNM",
    exchange: "HOSE",
    reference: "62500",
    ceiling: "66800",
    floor: "58200",
    bids: [level("62400", "1200"), level("62300", "800"), level("58200", "5000")],
    asks: [level("62600", "900"), level("62700", "300")],
    last: "63100",
    last_quantity: "300",
    change: "600",
    change_pct: "0.96",
    volume: "125400",
    high: "66800",
    low: "62500",
    ...overrides,
  };
}

describe("tones", () => {
  it("place a price against reference, ceiling and floor", () => {
    const bands = bandsOf(vnm());

    expect(toneOf(66800, bands)).toBe("ceiling");
    expect(toneOf(58200, bands)).toBe("floor");
    expect(toneOf(62500, bands)).toBe("ref");
    expect(toneOf(62600, bands)).toBe("up");
    expect(toneOf(62400, bands)).toBe("down");
  });

  it("are absent without a reference or a price", () => {
    expect(toneOf(100, bandsOf(vnm({ reference: null })))).toBeUndefined();
    expect(toneOf(null, bandsOf(vnm()))).toBeUndefined();
  });

  it("fall back to up and down where a market has no limits", () => {
    const bands = bandsOf(vnm({ ceiling: null, floor: null }));

    expect(toneOf(99999, bands)).toBe("up");
    expect(toneOf(1, bands)).toBe("down");
  });
});

describe("a board row", () => {
  it("scales prices for display but tones them on the exact values", () => {
    const row = boardRow(vnm(), 1000);

    expect(row.ref).toBe(62.5);
    expect(row.ceiling).toBe(66.8);
    expect(row.last).toBe(63.1);
    expect(row.change).toBe(0.6);
    expect(row.pct).toBe(0.96);
    expect(row.volume).toBe(125400);
    expect(row.tones.last).toBe("up");
    expect(row.tones.high).toBe("ceiling");
    expect(row.tones.low).toBe("ref");
  });

  it("has three levels a side, best first, with empty ones blank", () => {
    const row = boardRow(vnm());

    expect(BOARD_DEPTH).toBe(3);
    expect([row.b1, row.b1v, row.b3, row.b3v]).toEqual([62400, 1200, 58200, 5000]);
    expect(row.tones.b1).toBe("down");
    expect(row.tones.b3).toBe("floor");
    expect([row.a2, row.a3, row.a3v]).toEqual([62700, null, null]);
    expect(row.tones.a3).toBeUndefined();
  });

  it("is blank before the first trade", () => {
    const row = boardRow(vnm({ last: null, last_quantity: null, change: null, change_pct: null, high: null, low: null }));

    expect([row.last, row.lastv, row.change, row.pct, row.high]).toEqual([null, null, null, null, null]);
    expect(row.tones.last).toBeUndefined();
  });
});

describe("the board", () => {
  const boards = {
    VNM: vnm(),
    ACB: vnm({ symbol: "ACB" }),
    SHS: vnm({ symbol: "SHS", exchange: "HNX" }),
  };

  it("lists rows by symbol, filtered by exchange", () => {
    expect(boardRows(boards).map((r) => r.symbol)).toEqual(["ACB", "SHS", "VNM"]);
    expect(boardRows(boards, { exchange: "HNX" }).map((r) => r.symbol)).toEqual(["SHS"]);
  });

  it("offers each exchange once, alphabetically", () => {
    const instruments = {
      ACB: { exchange: "HOSE" },
      ACV: { exchange: "UPCOM" },
      SHS: { exchange: "HNX" },
      VNM: { exchange: "HOSE" },
      XYZ: { exchange: "" },
    };

    expect(exchangesOf(instruments)).toEqual(["HNX", "HOSE", "UPCOM"]);
  });
});
