import { describe, expect, it } from "vitest";
import { createSync } from "./sync.js";

const batch = (seq) => ({ seq, quotes: [], orders: [{ id: 1, seq }], account: null, bars: [] });
const snapshot = (seq) => ({ seq, quotes: [], orders: [{ id: 1, seq: "snapshot" }], account: { cash: "0", positions: [] } });

function recorder() {
  const applied = [];
  return { applied, sync: createSync((b) => applied.push(b.orders[0]?.seq ?? b.seq)) };
}

describe("createSync", () => {
  it("holds batches until the snapshot and drops those it already covers", () => {
    const { applied, sync } = recorder();

    sync.batch(batch(4));
    sync.batch(batch(6));
    sync.batch(batch(5));
    sync.snapshot(snapshot(5));

    expect(applied).toEqual(["snapshot", 6]);
  });

  it("passes newer batches straight through once synced", () => {
    const { applied, sync } = recorder();
    sync.snapshot(snapshot(2));

    sync.batch(batch(3));
    sync.batch(batch(4));

    expect(applied).toEqual(["snapshot", 3, 4]);
  });

  it("ignores a batch older than the last one applied", () => {
    const { applied, sync } = recorder();
    sync.snapshot(snapshot(2));
    sync.batch(batch(4));

    sync.batch(batch(3));

    expect(applied).toEqual(["snapshot", 4]);
  });

  it("hands on the snapshot's reference data and board rows as a batch", () => {
    const received = [];
    const sync = createSync((b) => received.push(b));

    sync.snapshot({ ...snapshot(1), instruments: [{ symbol: "VNM" }], boards: [{ symbol: "VNM" }] });
    sync.snapshot(snapshot(2));

    expect(received[0].instruments).toEqual([{ symbol: "VNM" }]);
    expect(received[0].boards).toEqual([{ symbol: "VNM" }]);
    expect(received[1].instruments).toEqual([]);
  });

  it("hands on the snapshot's connections as a batch", () => {
    const received = [];
    const sync = createSync((b) => received.push(b));
    const orders = { connection: "orders", state: "reconnecting", detail: "stream closed" };

    sync.snapshot({ ...snapshot(1), connections: [orders] });
    sync.snapshot(snapshot(2));

    expect(received[0].connections).toEqual([orders]);
    expect(received[1].connections).toEqual([]);
  });

  it("hands on the snapshot's sessions as a batch", () => {
    const received = [];
    const sync = createSync((b) => received.push(b));
    const hose = { market: "HOSE", phase: "openingAuction", name: "ATO", order_types: [] };

    sync.snapshot({ ...snapshot(1), sessions: [hose] });
    sync.snapshot(snapshot(2));

    expect(received[0].sessions).toEqual([hose]);
    expect(received[1].sessions).toEqual([]);
  });
});
