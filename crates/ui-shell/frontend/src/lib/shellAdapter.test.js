import { beforeEach, describe, expect, it, vi } from "vitest";

const subscribers = new Set();

vi.mock("./bridge.js", () => ({
  onBatch: (subscriber) => {
    subscribers.add(subscriber);
    return () => subscribers.delete(subscriber);
  },
}));

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...args) => invoke(...args) }));

const { ShellAdapter, symbolInfoOf, toChartBar } = await import("./shellAdapter.js");

const deliver = (batch) => {
  for (const subscriber of subscribers) {
    subscriber({ quotes: [], orders: [], account: null, bars: [], ...batch });
  }
};

const bar = (symbol, time_ms, close) => ({
  symbol,
  time_ms,
  open: "10",
  high: "11",
  low: "9",
  close,
  ticks: 4,
});

function connected(symbol = "AAA") {
  const adapter = new ShellAdapter();
  const events = [];
  for (const type of ["bar", "tick", "connectionChange"]) {
    adapter.on(type, (event) => events.push(event));
  }
  adapter.connect({ symbol, timeframe: "5s" });
  return { adapter, events };
}

beforeEach(() => {
  subscribers.clear();
  invoke.mockReset();
});

describe("ShellAdapter", () => {
  it("reports itself connected", () => {
    const { adapter, events } = connected();

    expect(adapter.getConnectionState()).toBe("connected");
    expect(events.at(-1)).toMatchObject({ type: "connectionChange", data: "connected" });
  });

  it("loads history through the shell, never the network", async () => {
    invoke.mockResolvedValue([bar("AAA", 5000, "10.5")]);
    const adapter = new ShellAdapter();

    const history = await adapter.fetchHistory("AAA", "5s", 300);

    expect(invoke).toHaveBeenCalledWith("bars", { symbol: "AAA", limit: 300 });
    expect(history).toEqual([toChartBar(bar("AAA", 5000, "10.5"))]);
  });

  const barEvents = (events) =>
    events.filter((e) => e.type === "bar").map((e) => [e.data.bar.time, e.data.bar.close, e.data.closed]);

  it("closes a bar with its final state when a later one arrives in the same batch", () => {
    const { events } = connected();

    deliver({ bars: [bar("AAA", 0, "10"), bar("AAA", 5000, "10.2")] });

    expect(barEvents(events)).toEqual([
      [0, 10, false],
      [0, 10, true],
      [5000, 10.2, false],
    ]);
  });

  it("closes a bar whose final state went out in an earlier batch", () => {
    const { events } = connected();

    deliver({ bars: [bar("AAA", 0, "10.1")] });
    deliver({ bars: [bar("AAA", 5000, "10.3")] });

    expect(barEvents(events)).toEqual([
      [0, 10.1, false],
      [0, 10.1, true],
      [5000, 10.3, false],
    ]);
  });

  it("keeps a bar forming while it is still updating", () => {
    const { events } = connected();

    deliver({ bars: [bar("AAA", 0, "10")] });
    deliver({ bars: [bar("AAA", 0, "10.4")] });

    expect(barEvents(events)).toEqual([
      [0, 10, false],
      [0, 10.4, false],
    ]);
  });

  it("ignores other symbols", () => {
    const { events } = connected("AAA");

    deliver({
      bars: [bar("BBB", 0, "1")],
      quotes: [{ symbol: "BBB", bid: "1", ask: "1.02", ts_ms: 0 }],
    });

    expect(events.filter((e) => e.type !== "connectionChange")).toEqual([]);
  });

  it("turns its symbol's quote into a mid-price tick", () => {
    const { events } = connected();

    deliver({ quotes: [{ symbol: "AAA", bid: "10.00", ask: "10.02", ts_ms: 1234 }] });

    const tick = events.find((e) => e.type === "tick").data;
    expect(tick).toMatchObject({ time: 1234, bid: 10, ask: 10.02 });
    expect(tick.price).toBeCloseTo(10.01);
  });

  it("stops listening when disconnected", () => {
    const { adapter, events } = connected();

    adapter.disconnect();
    deliver({ bars: [bar("AAA", 0, "10")] });

    expect(events.filter((e) => e.type === "bar")).toEqual([]);
    expect(adapter.getConnectionState()).toBe("disconnected");
  });

  it("announces an instrument's details to a chart already connected to it", () => {
    const adapter = new ShellAdapter();
    const heard = [];
    adapter.on("symbolInfo", (event) => heard.push(event.data));
    const vnm = {
      symbol: "VNM",
      name: "Vinamilk",
      exchange: "HOSE",
      currency: "VND",
      ticks: [{ from: "0", tick: "10" }],
    };

    adapter.announce(vnm);
    adapter.connect({ symbol: "VNM" });
    adapter.announce({ ...vnm, symbol: "FPT" });
    adapter.announce(vnm);

    expect(heard).toEqual([expect.objectContaining({ symbol: "VNM", pricePrecision: 0, minTick: 10 })]);
  });

  it("tells the chart each instrument's names and price steps", async () => {
    const vnm = {
      symbol: "VNM",
      name: "Vinamilk",
      exchange: "HOSE",
      currency: "VND",
      lot_size: "100",
      ticks: [
        { from: "0", tick: "10" },
        { from: "10000", tick: "50" },
        { from: "50000", tick: "100" },
      ],
    };
    const adapter = new ShellAdapter((symbol) => (symbol === "VNM" ? vnm : undefined));

    expect(await adapter.resolveSymbol("VNM")).toMatchObject({
      description: "Vinamilk",
      exchange: "HOSE",
      pricePrecision: 0,
      minTick: 10,
      currency: "VND",
    });
    expect(await adapter.resolveSymbol("XYZ")).toBeNull();
    expect(symbolInfoOf({ symbol: "AAPL", name: "Apple", exchange: "", currency: "USD", ticks: [{ from: "0", tick: "0.01" }] }))
      .toMatchObject({ pricePrecision: 2, minTick: 0.01, exchange: undefined });
  });
});
