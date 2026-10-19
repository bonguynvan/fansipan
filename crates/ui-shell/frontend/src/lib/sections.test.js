import { beforeEach, describe, expect, it } from "vitest";
import {
  TRADE,
  getSection,
  listSections,
  place,
  registerSection,
  resetSectionsForTests,
  revealFor,
  showSection,
} from "./sections.svelte.js";
import { hotkeys, runAction } from "./hotkeys.svelte.js";

const component = {};

describe("sections", () => {
  beforeEach(resetSectionsForTests);

  it("keeps Trade first and the rest in order, each as an action", async () => {
    registerSection({ id: "help", title: "section.help", icon: "M0 0", component, order: 90 });
    registerSection({ id: TRADE, title: "section.trade", icon: "M0 0", order: 99 });
    registerSection({ id: "settings", title: "section.settings", icon: "M0 0", component, order: 80 });

    expect(listSections().map((s) => s.id)).toEqual([TRADE, "settings", "help"]);
    expect(hotkeys.actions.some((a) => a.id === "section.help")).toBe(true);

    await runAction("section.help");
    expect(place.current).toBe("help");
  });

  it("shows only a registered section", () => {
    registerSection({ id: "help", title: "section.help", icon: "M0 0", component });

    showSection("inbox");
    expect(place.current).toBe(TRADE);
    showSection("help");
    expect(place.current).toBe("help");
    expect(getSection("help").title).toBe("section.help");
  });

  it("shows the workspace for an action that works on it, and not for the app's own", () => {
    registerSection({ id: TRADE, title: "section.trade", icon: "M0 0" });
    registerSection({ id: "help", title: "section.help", icon: "M0 0", component });
    for (const id of ["app.search", "trading.halt", "section.settings"]) {
      showSection("help");
      revealFor(id);
      expect(place.current, id).toBe("help");
    }
    for (const id of ["workspace.close-page", "ticket.buy", "orders.cancel-all", "plugin.thing"]) {
      showSection("help");
      revealFor(id);
      expect(place.current, id).toBe(TRADE);
    }
  });

  it("refuses a section with nothing to show, or a bad or repeated id", () => {
    expect(() => registerSection({ id: "inbox", title: "section.inbox", icon: "M0 0" })).toThrow(/component/);
    expect(() => registerSection({ id: "Bad id", title: "x", icon: "", component })).toThrow(/id/);
    registerSection({ id: "help", title: "section.help", icon: "M0 0", component });
    expect(() => registerSection({ id: "help", title: "section.help", icon: "M0 0", component })).toThrow(
      /already/,
    );
  });
});
