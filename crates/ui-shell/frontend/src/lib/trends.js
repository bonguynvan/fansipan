// Watchlist sparklines: the last few bars per symbol, as bo-grid candles.

/** Bars per watchlist sparkline: two minutes of 5-second bars. */
export const TREND_BARS = 24;

/** A shell bar (decimal strings) as a sparkline candle (numbers, display only). */
export function toCandle(bar) {
  return {
    time: bar.time_ms,
    open: Number(bar.open),
    high: Number(bar.high),
    low: Number(bar.low),
    close: Number(bar.close),
    volume: bar.ticks,
  };
}

/** The trend with `candle` folded in: replaces the candle at its time, or appends and trims. Never mutates. */
export function withCandle(trend, candle) {
  const last = trend.at(-1);
  if (last && last.time === candle.time) return [...trend.slice(0, -1), candle];
  if (last && last.time > candle.time) return trend;
  return [...trend, candle].slice(-TREND_BARS);
}

/** Every trend with this batch's bars folded in. */
export function withBars(trends, bars) {
  return bars.reduce(
    (next, bar) => ({ ...next, [bar.symbol]: withCandle(next[bar.symbol] ?? [], toCandle(bar)) }),
    trends,
  );
}

/**
 * Loaded history merged with live candles that arrived while it loaded:
 * the live ones are folded on top, so neither replaces the other.
 */
export function withHistory(trends, loaded) {
  const merged = loaded.map(([symbol, history]) => [
    symbol,
    (trends[symbol] ?? []).reduce(withCandle, history),
  ]);
  return { ...trends, ...Object.fromEntries(merged) };
}
