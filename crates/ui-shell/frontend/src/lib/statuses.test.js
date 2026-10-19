import { describe, expect, it } from "vitest";
import { STATUS_WORDS, boardMark, statusNotice, withStatuses } from "./statuses.js";
import english from "../i18n/locales/en.json";
import vietnamese from "../i18n/locales/vi.json";

const status = (fields) => ({
  symbol: "AAA",
  trading: "trading",
  takes_orders: true,
  short_sale_restricted: false,
  reason: null,
  ...fields,
});

describe("instrument statuses", () => {
  it("keeps each symbol's latest, and keeps what was there when nothing is reported", () => {
    const halted = status({ trading: "halted", takes_orders: false });
    const current = withStatuses({}, [halted]);

    expect(current).toEqual({ AAA: halted });
    expect(withStatuses(current, [])).toBe(current);
    expect(withStatuses(current, [status({})]).AAA.trading).toBe("trading");
  });

  it("marks a stopped instrument on the board before a restricted one, and a free one not at all", () => {
    expect(boardMark(undefined)).toBeNull();
    expect(boardMark(status({}))).toBeNull();
    expect(boardMark(status({ reason: "under warning" }))).toBeNull();
    expect(boardMark(status({ short_sale_restricted: true }))).toBe("status.short.restricted");
    expect(boardMark(status({ trading: "halted", short_sale_restricted: true }))).toBe("status.short.halted");
    expect(boardMark(status({ trading: "suspended" }))).toBe("status.short.suspended");
    expect(boardMark(status({ trading: "unknown" }))).toBe("status.short.halted");
  });

  it("tells the ticket whether orders are taken, and why, when the venue says", () => {
    expect(statusNotice(undefined, "AAA")).toBeNull();
    expect(statusNotice(status({}), "AAA")).toBeNull();
    expect(statusNotice(status({ trading: "halted", takes_orders: false }), "AAA")).toEqual({
      id: "ticket.halted",
      vars: { symbol: "AAA", reason: "" },
    });
    expect(statusNotice(status({ trading: "halted", takes_orders: true, reason: "LULD" }), "AAA").id).toBe(
      "ticket.haltedQueuedWhy",
    );
    expect(statusNotice(status({ trading: "suspended", takes_orders: false, reason: "filing late" }), "AAA").id).toBe(
      "ticket.suspendedWhy",
    );
    expect(statusNotice(status({ short_sale_restricted: true }), "AAA").id).toBe("ticket.shortRestricted");
    expect(statusNotice(status({ reason: "under warning" }), "AAA")).toEqual({
      id: "ticket.statusReason",
      vars: { symbol: "AAA", reason: "under warning" },
    });
  });

  it("has every word in every language, with the values it is given", () => {
    for (const words of [english, vietnamese]) {
      for (const id of STATUS_WORDS) {
        expect(words[id], id).toBeTruthy();
        if (id.startsWith("ticket.")) expect(words[id], id).toContain("{symbol}");
        if (id.endsWith("Why") || id === "ticket.statusReason") expect(words[id], id).toContain("{reason}");
      }
    }
  });
});
