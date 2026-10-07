import { describe, expect, it } from "vitest";
import { TREND_BARS, toCandle, withBars, withCandle, withHistory } from "./trends.js";

const bar = (symbol, time_ms, close) => ({
  symbol,
  time_ms,
  open: "1",
  high: "2",
  low: "0.5",
  close,
  ticks: 3,
});

const candle = (time, close) => toCandle(bar("AAA", time, String(close)));

describe("toCandle", () => {
  it("turns decimal strings into numbers and ticks into volume", () => {
    expect(toCandle(bar("AAA", 5000, "1.25"))).toEqual({
      time: 5000,
      open: 1,
      high: 2,
      low: 0.5,
      close: 1.25,
      volume: 3,
    });
  });
});

describe("withCandle", () => {
  it("replaces the candle for the same interval", () => {
    const trend = [candle(0, 1), candle(5000, 2)];

    const next = withCandle(trend, candle(5000, 3));

    expect(next.map((c) => c.close)).toEqual([1, 3]);
    expect(trend.map((c) => c.close)).toEqual([1, 2]);
  });

  it("ignores a candle older than the newest one", () => {
    const trend = [candle(5000, 2)];

    expect(withCandle(trend, candle(0, 9))).toBe(trend);
  });

  it("keeps only the newest TREND_BARS candles", () => {
    const full = Array.from({ length: TREND_BARS }, (_, i) => candle(i * 5000, i));

    const next = withCandle(full, candle(TREND_BARS * 5000, 99));

    expect(next).toHaveLength(TREND_BARS);
    expect(next.at(-1).close).toBe(99);
    expect(next[0].time).toBe(5000);
  });
});

describe("withBars", () => {
  it("folds each bar into its own symbol's trend", () => {
    const next = withBars({}, [bar("AAA", 0, "1"), bar("BBB", 0, "2"), bar("AAA", 5000, "3")]);

    expect(next.AAA.map((c) => c.close)).toEqual([1, 3]);
    expect(next.BBB.map((c) => c.close)).toEqual([2]);
  });
});

describe("withHistory", () => {
  it("keeps live candles that arrived while the history was loading", () => {
    const live = { AAA: [candle(15000, 7)] };
    const history = [candle(0, 1), candle(5000, 2), candle(10000, 3)];

    const next = withHistory(live, [["AAA", history]]);

    expect(next.AAA.map((c) => c.time)).toEqual([0, 5000, 10000, 15000]);
  });

  it("lets a live candle replace the history's last one for the same interval", () => {
    const live = { AAA: [candle(10000, 9)] };
    const history = [candle(5000, 2), candle(10000, 3)];

    const next = withHistory(live, [["AAA", history]]);

    expect(next.AAA.map((c) => c.close)).toEqual([2, 9]);
  });

  it("leaves symbols it did not load alone", () => {
    const live = { BBB: [candle(0, 4)] };

    expect(withHistory(live, [["AAA", []]]).BBB).toEqual(live.BBB);
  });
});
