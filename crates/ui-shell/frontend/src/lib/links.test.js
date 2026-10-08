import { beforeEach, describe, expect, it, vi } from "vitest";

const emitted = [];
vi.mock("@tauri-apps/api/event", () => ({
  emit: async (event, payload) => emitted.push({ event, payload }),
  listen: async () => () => {},
}));
vi.mock("./settings.js", () => ({
  debouncedSetter: () => Object.assign(() => {}, { flush: async () => {} }),
  getSetting: async () => null,
}));

const { chooseSymbol, isLinked, link, linkPatch, migrateLinkParams, setSharedSymbol, symbolOf } = await import(
  "./links.svelte.js"
);

beforeEach(() => {
  emitted.length = 0;
  link.symbol = null;
});

describe("symbolOf", () => {
  it("follows the shared symbol when linked", () => {
    link.symbol = "MSFT";

    expect(symbolOf({ linked: true, symbol: "AAPL" }, "TSLA")).toBe("MSFT");
  });

  it("keeps its own symbol when not linked", () => {
    link.symbol = "MSFT";

    expect(symbolOf({ linked: false, symbol: "AAPL" }, "TSLA")).toBe("AAPL");
  });

  it("falls back when there is nothing to show yet", () => {
    expect(symbolOf({ linked: true }, "TSLA")).toBe("TSLA");
    expect(symbolOf(undefined, "TSLA")).toBe("TSLA");
  });

  it("passes over a symbol the market does not list, as one saved on another market", () => {
    link.symbol = "NVDA";
    const listed = ["FPT", "VNM"];

    expect(symbolOf({ linked: true }, "FPT", listed)).toBe("FPT");
    expect(symbolOf({ linked: false, symbol: "AAPL" }, "FPT", listed)).toBe("FPT");
    expect(symbolOf({ linked: true }, null, listed)).toBeNull();
  });

  it("keeps a listed symbol, and any symbol while the market's list is not in yet", () => {
    link.symbol = "VNM";

    expect(symbolOf({ linked: true }, "FPT", ["FPT", "VNM"])).toBe("VNM");
    expect(symbolOf({ linked: true }, "FPT", [])).toBe("VNM");
    expect(symbolOf({ linked: false, symbol: "VNM" }, "FPT", ["FPT", "VNM"])).toBe("VNM");
  });

  it("falls back when nothing has been picked, whatever the market lists", () => {
    expect(symbolOf({ linked: true }, "FPT", ["FPT", "VNM"])).toBe("FPT");
    expect(symbolOf({ linked: false }, "FPT", ["FPT", "VNM"])).toBe("FPT");
  });
});

describe("choosing a symbol", () => {
  it("sets the shared symbol from a linked panel and tells the other windows", () => {
    chooseSymbol({ linked: true }, null, "NVDA");

    expect(link.symbol).toBe("NVDA");
    expect(emitted).toHaveLength(1);
    expect(emitted[0].event).toBe("link-symbol");
    expect(emitted[0].payload.symbol).toBe("NVDA");
  });

  it("keeps an unlinked panel's pick to itself", () => {
    const updates = [];
    const panel = { updateParameters: (patch) => updates.push(patch) };

    chooseSymbol({ linked: false }, panel, "NVDA");

    expect(updates).toEqual([{ symbol: "NVDA" }]);
    expect(link.symbol).toBeNull();
    expect(emitted).toEqual([]);
  });

  it("orders picks so a newer one is never undone by an older one", () => {
    setSharedSymbol("NVDA");
    setSharedSymbol("TSLA");

    expect(emitted[1].payload.at).toBeGreaterThan(emitted[0].payload.at);
  });

  it("does not resend an unchanged symbol", () => {
    setSharedSymbol("NVDA");
    setSharedSymbol("NVDA");
    setSharedSymbol(null);

    expect(emitted).toHaveLength(1);
  });
});

describe("linking and unlinking", () => {
  it("unlinks keeping the symbol on screen, so nothing jumps", () => {
    link.symbol = "VNM";

    expect(linkPatch({ linked: true }, false)).toEqual({ linked: false, symbol: "VNM" });
    expect(linkPatch({ linked: false, symbol: "FPT" }, true)).toEqual({ linked: true });
    expect(isLinked({ linked: true })).toBe(true);
    expect(isLinked({})).toBe(false);
  });
});

describe("migrateLinkParams", () => {
  it("turns a link group into linked, and none into not", () => {
    expect(migrateLinkParams({ link: "b", list: "x" })).toEqual({ linked: true, list: "x" });
    expect(migrateLinkParams({ link: "none", symbol: "FPT" })).toEqual({ linked: false, symbol: "FPT" });
  });

  it("leaves parameters without a group alone", () => {
    const params = { linked: true };

    expect(migrateLinkParams(params)).toBe(params);
    expect(migrateLinkParams(undefined)).toBeUndefined();
  });
});
