import { describe, expect, it } from "vitest";
import english from "../i18n/locales/en.json";
import { journalNotice } from "./journal.js";

const kept = (patch = {}) => ({
  kept: true,
  restored_orders: 3,
  restored_conditionals: 1,
  unconfirmed: 0,
  venue_asked: true,
  damaged: null,
  error: null,
  ...patch,
});

describe("the journal's notice at start", () => {
  it("says the most pressing thing", () => {
    expect(journalNotice(kept({ error: "disk full", damaged: "x.damaged" })).id).toBe("journal.failed");
    expect(journalNotice(kept({ damaged: "x.damaged" }))).toEqual({ id: "journal.damaged", vars: { path: "x.damaged" } });
    expect(journalNotice(kept({ damaged: "x.damaged", unconfirmed: 2, venue_asked: false }))).toEqual({
      id: "journal.damagedUnconfirmed",
      vars: { path: "x.damaged", unconfirmed: 2 },
    });
    expect(journalNotice(kept({ unconfirmed: 2, venue_asked: false }))).toEqual({
      id: "journal.restoredUnconfirmed",
      vars: { orders: 3, held: 1, unconfirmed: 2 },
    });
    expect(journalNotice(kept({ unconfirmed: 2 }))).toEqual({ id: "journal.restored", vars: { orders: 3, held: 1 } });
  });

  it("says nothing when nothing came back or no journal is kept", () => {
    expect(journalNotice(kept({ restored_orders: 0, restored_conditionals: 0 }))).toBeNull();
    expect(journalNotice({ kept: false, restored_orders: 0, restored_conditionals: 0 })).toBeNull();
    expect(journalNotice(null)).toBeNull();
  });

  it("has words for everything it can say", () => {
    for (const id of [
      "journal.failed",
      "journal.damaged",
      "journal.damagedUnconfirmed",
      "journal.restored",
      "journal.restoredUnconfirmed",
      "journal.writeFailed",
      "status.unconfirmed",
      "order.unconfirmedReason",
      "conditional.lifetimeKept",
    ]) {
      expect(english[id]).toBeTruthy();
    }
  });
});
