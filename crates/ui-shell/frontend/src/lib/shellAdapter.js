// A TradeCanvas DataAdapter backed by the shell instead of the network.
// History comes from the `bars` command, live bars and ticks from the bus
// bridge. The chart never opens a connection of its own, so it shows
// exactly what the engines see, whichever venue adapter the shell runs.

import { invoke } from "@tauri-apps/api/core";
import { onBatch } from "./bridge.js";
import { finestTick, tickDecimals, tickTable } from "./ticks.js";

/** The interval data-engine builds bars at. The chart resamples longer timeframes from it. */
export const BAR_TIMEFRAME = "5s";

/** A shell bar (decimal strings) as the chart's OHLC bar (numbers, display only). */
export function toChartBar(bar) {
  return {
    time: bar.time_ms,
    open: Number(bar.open),
    high: Number(bar.high),
    low: Number(bar.low),
    close: Number(bar.close),
    volume: bar.ticks,
  };
}

/**
 * What the chart is told about an instrument (the shell's instrument row):
 * its names, and the price steps that set the axis precision, so VND shows
 * no decimals and cents show two. Null when the instrument is not known yet.
 */
export function symbolInfoOf(instrument) {
  if (!instrument) return null;
  const ticks = tickTable(instrument);
  return {
    symbol: instrument.symbol,
    description: instrument.name,
    exchange: instrument.exchange || undefined,
    type: "stock",
    pricePrecision: tickDecimals(ticks),
    minTick: finestTick(ticks),
    currency: instrument.currency || undefined,
  };
}

export class ShellAdapter {
  name = "fansipan-shell";
  supportedTimeframes = [BAR_TIMEFRAME];

  #describe;
  #state = "disconnected";
  #listeners = new Map();
  #symbol = null;
  #unsubscribe = null;
  /** The newest bar sent and not yet closed. */
  #forming = null;

  /** `describe(symbol)` returns the instrument row the shell sent for it, if any. */
  constructor(describe = () => null) {
    this.#describe = describe;
  }

  connect(config) {
    this.#stop();
    this.#symbol = config.symbol;
    this.#forming = null;
    this.#unsubscribe = onBatch((batch) => this.#apply(batch));
    this.#setState("connected");
  }

  disconnect() {
    this.#stop();
    this.#setState("disconnected");
  }

  getConnectionState() {
    return this.#state;
  }

  async resolveSymbol(symbol) {
    return symbolInfoOf(this.#describe(symbol));
  }

  /**
   * Tells a connected chart its symbol's details once they are known (the
   * instrument row the shell sent), so it applies their price precision.
   * Saying the same again changes nothing.
   */
  announce(instrument) {
    if (this.#state !== "connected" || instrument?.symbol !== this.#symbol) return;
    const info = symbolInfoOf(instrument);
    if (info) this.#emit("symbolInfo", info);
  }

  async fetchHistory(symbol, _timeframe, limit) {
    const bars = await invoke("bars", { symbol, limit: limit ?? null });
    return bars.map(toChartBar);
  }

  on(type, listener) {
    if (!this.#listeners.has(type)) this.#listeners.set(type, new Set());
    this.#listeners.get(type).add(listener);
  }

  off(type, listener) {
    this.#listeners.get(type)?.delete(listener);
  }

  dispose() {
    this.#stop();
    this.#listeners.clear();
  }

  #stop() {
    this.#unsubscribe?.();
    this.#unsubscribe = null;
  }

  // Bars arrive in time order. A bar is closed once a later one exists,
  // which may be in this batch or a later one, so the bar still forming is
  // remembered across batches and closed with its last known state.
  #apply(batch) {
    for (const bar of batch.bars.filter((b) => b.symbol === this.#symbol)) {
      const next = toChartBar(bar);
      if (this.#forming && next.time > this.#forming.time) {
        this.#emit("bar", { bar: this.#forming, closed: true });
      }
      this.#forming = next;
      this.#emit("bar", { bar: next, closed: false });
    }

    const quote = batch.quotes.find((q) => q.symbol === this.#symbol);
    if (quote) {
      const bid = Number(quote.bid);
      const ask = Number(quote.ask);
      this.#emit("tick", { time: quote.ts_ms, price: (bid + ask) / 2, volume: 0, bid, ask });
    }
  }

  #emit(type, data) {
    for (const listener of this.#listeners.get(type) ?? []) {
      listener({ type, data, timestamp: Date.now() });
    }
  }

  #setState(state) {
    if (this.#state === state) return;
    this.#state = state;
    this.#emit("connectionChange", state);
  }
}
