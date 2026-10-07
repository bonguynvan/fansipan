import { beforeEach, describe, expect, it, vi } from "vitest";

const emitted = [];
vi.mock("@tauri-apps/api/event", () => ({
  emit: (event, payload) => emitted.push({ event, payload }),
  listen: async () => () => {},
}));
vi.mock("./settings.js", () => ({
  debouncedSetter: () => () => {},
  getSetting: async () => null,
}));

const { LINK_GROUPS, UNLINKED, isLinked, links, nextGroup, selectSymbol, symbolOf } = await import("./links.svelte.js");

beforeEach(() => {
  emitted.length = 0;
  for (const group of LINK_GROUPS) links.symbols[group.id] = null;
});

describe("nextGroup", () => {
  it("cycles A, B, C, none, and back to A", () => {
    const seen = [];
    let group = "a";
    for (let i = 0; i < 5; i++) {
      seen.push(group);
      group = nextGroup(group);
    }
    expect(seen).toEqual(["a", "b", "c", UNLINKED, "a"]);
  });

  it("treats an unknown group as unlinked", () => {
    expect(nextGroup(undefined)).toBe("a");
    expect(isLinked("z")).toBe(false);
  });
});

describe("symbolOf", () => {
  it("follows the group when linked", () => {
    links.symbols.b = "MSFT";

    expect(symbolOf({ link: "b", symbol: "AAPL" }, "TSLA")).toBe("MSFT");
  });

  it("keeps its own symbol when unlinked", () => {
    links.symbols.a = "MSFT";

    expect(symbolOf({ link: UNLINKED, symbol: "AAPL" }, "TSLA")).toBe("AAPL");
  });

  it("falls back when the group has no symbol yet", () => {
    expect(symbolOf({ link: "a" }, "TSLA")).toBe("TSLA");
    expect(symbolOf(undefined, "TSLA")).toBe("TSLA");
  });
});

describe("selectSymbol", () => {
  it("sets the group and tells the other windows", () => {
    selectSymbol("a", "NVDA");

    expect(links.symbols.a).toBe("NVDA");
    expect(emitted).toEqual([{ event: "link-select", payload: { group: "a", symbol: "NVDA" } }]);
  });

  it("does nothing for an unlinked panel or an unchanged symbol", () => {
    selectSymbol(UNLINKED, "NVDA");
    selectSymbol("a", null);

    expect(emitted).toEqual([]);
  });
});
