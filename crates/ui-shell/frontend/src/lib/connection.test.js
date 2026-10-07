import { describe, expect, it } from "vitest";
import { connectionLines, summarize, withConnections } from "./connection.js";

const up = (connection) => ({ connection, state: "up", detail: null });
const at = (connection, state, detail = null) => ({ connection, state, detail });
const keyed = (...connections) => withConnections({}, connections);

describe("connections", () => {
  it("reads as up when every connection is, or none was reported", () => {
    expect(summarize({})).toEqual({ state: "up", connection: null });
    expect(summarize(keyed(up("marketData"), up("orders")))).toEqual({ state: "up", connection: null });
  });

  it("names the connection in the worst state", () => {
    const connections = keyed(at("marketData", "degraded"), at("orders", "reconnecting", "stream closed"));

    expect(summarize(connections)).toEqual({ state: "reconnecting", connection: "orders" });
  });

  it("names no connection when all of them are in the worst state", () => {
    expect(summarize(keyed(at("marketData", "down"), at("orders", "down")))).toEqual({
      state: "down",
      connection: null,
    });
  });

  it("keeps the latest report per connection", () => {
    const before = keyed(at("orders", "reconnecting"), up("marketData"));

    const after = withConnections(before, [up("orders")]);

    expect(summarize(after)).toEqual({ state: "up", connection: null });
    expect(withConnections(after, [])).toBe(after);
  });

  it("treats a state or a connection it has no name for as unknown", () => {
    const connections = keyed(up("marketData"), at("orders", "stale"), at("history", "down"));

    expect(connectionLines(connections)).toEqual([
      up("marketData"),
      at("orders", "unknown"),
      at("unknown", "down"),
    ]);
  });
});
