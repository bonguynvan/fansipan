import { describe, expect, it } from "vitest";
import {
  DEFAULT_ORDER_TYPES,
  isPriced,
  orderTypesFor,
  pickOrderType,
  typeKey,
  venueNameOf,
  withSessions,
  withTypeNames,
} from "./sessions.js";

const lo = { kind: "limit", time_in_force: "day", name: "LO" };
const ato = { kind: "market", time_in_force: "atOpen", name: "ATO" };
const mtl = { kind: "marketToLimit", time_in_force: "day", name: "MTL" };
const session = (market, phase, order_types, name = null) => ({ market, phase, name, order_types });

describe("sessions", () => {
  it("keep the latest session per market", () => {
    const opening = withSessions({}, [session("HOSE", "openingAuction", [ato, lo], "ATO")]);

    const later = withSessions(opening, [session("HOSE", "continuous", [lo, mtl])]);

    expect(later.HOSE.phase).toBe("continuous");
    expect(withSessions(later, [])).toBe(later);
  });

  it("offer a market's own types, or the defaults when it has no session", () => {
    const sessions = withSessions({}, [session("HOSE", "openingAuction", [ato, lo]), session("HNX", "closed", [])]);

    expect(orderTypesFor("HOSE", sessions)).toEqual([ato, lo]);
    expect(orderTypesFor("HNX", sessions)).toEqual([]);
    expect(orderTypesFor("NASDAQ", sessions)).toBe(DEFAULT_ORDER_TYPES);
    expect(orderTypesFor("", sessions)).toBe(DEFAULT_ORDER_TYPES);
  });

  it("keep the chosen type while offered, else fall back to a limit order", () => {
    expect(pickOrderType([lo, mtl], typeKey(mtl))).toBe(mtl);
    expect(pickOrderType([ato, lo], typeKey(mtl))).toBe(lo);
    expect(pickOrderType([ato], typeKey(mtl))).toBe(ato);
    expect(pickOrderType([], typeKey(lo))).toBeNull();
  });

  it("price only limit orders", () => {
    expect(isPriced(lo)).toBe(true);
    expect(isPriced(mtl)).toBe(false);
    expect(isPriced(null)).toBe(false);
  });

  it("name an order's type as its market did, after the session moves on", () => {
    const morning = withTypeNames({}, [session("HOSE", "continuous", [lo, mtl])]);
    const lunch = withTypeNames(morning, [session("HOSE", "break", [lo])]);

    expect(venueNameOf({ kind: "marketToLimit", time_in_force: "day" }, "HOSE", lunch)).toBe("MTL");
    expect(venueNameOf({ kind: "market", time_in_force: "atOpen" }, "HOSE", lunch)).toBeNull();
    expect(venueNameOf({ kind: "limit", time_in_force: "day" }, "NASDAQ", lunch)).toBeNull();
    expect(withTypeNames(lunch, [])).toBe(lunch);
  });
});
