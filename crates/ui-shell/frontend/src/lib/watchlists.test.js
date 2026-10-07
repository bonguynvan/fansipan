import { describe, expect, it } from "vitest";
import {
  ALL,
  MAX_NAME,
  addProblem,
  cleanName,
  listsSetting,
  moved,
  nextListName,
  normalizeLists,
  renamed,
  symbolsOf,
  withList,
  withSymbol,
  withoutList,
  withoutSymbol,
} from "./watchlists.js";

const banks = { id: "l1", name: "Banks", symbols: ["VCB", "BID", "CTG"] };
const steel = { id: "l2", name: "Steel", symbols: ["HPG"] };
const lists = [banks, steel];

describe("listsSetting", () => {
  it("keeps each market's lists under its own key", () => {
    expect(listsSetting("vn")).toBe("screen.watchlist.lists.vn");
    expect(listsSetting("us")).not.toBe(listsSetting("vn"));
  });

  it("makes any market id safe for a settings key", () => {
    expect(listsSetting("Alpaca Paper/US")).toBe("screen.watchlist.lists.alpaca-paper-us");
    expect(listsSetting(null)).toBe("screen.watchlist.lists.default");
  });
});

describe("normalizeLists", () => {
  it("keeps well-formed lists as they are", () => {
    expect(normalizeLists(lists)).toEqual(lists);
  });

  it("drops what the store should not hold", () => {
    const saved = [
      banks,
      { id: "l1", name: "Duplicate id", symbols: [] },
      { id: ALL, name: "Shadows all", symbols: [] },
      { id: "l3", name: "   ", symbols: [] },
      { name: "No id" },
      null,
      { id: "l4", name: " Mixed ", symbols: ["hpg", " HPG ", 7, "", "NKG"] },
    ];

    expect(normalizeLists(saved)).toEqual([banks, { id: "l4", name: "Mixed", symbols: ["HPG", "NKG"] }]);
  });

  it("reads anything that is not a list of lists as none", () => {
    expect(normalizeLists(null)).toEqual([]);
    expect(normalizeLists({ lists })).toEqual([]);
  });
});

describe("editing", () => {
  it("adds a list with a cleaned name and no symbols", () => {
    expect(withList(lists, "l3", "  Real   estate ").at(-1)).toEqual({ id: "l3", name: "Real estate", symbols: [] });
  });

  it("renames, but not to nothing", () => {
    expect(renamed(lists, "l2", "Metals")[1].name).toBe("Metals");
    expect(renamed(lists, "l2", "  ")).toBe(lists);
  });

  it("caps a name's length", () => {
    expect(cleanName("x".repeat(MAX_NAME + 10))).toHaveLength(MAX_NAME);
  });

  it("deletes a list", () => {
    expect(withoutList(lists, "l1")).toEqual([steel]);
  });

  it("adds a symbol once, cleaned", () => {
    expect(withSymbol(lists, "l2", " nkg ")[1].symbols).toEqual(["HPG", "NKG"]);
    expect(withSymbol(lists, "l2", "HPG")[1].symbols).toEqual(["HPG"]);
  });

  it("removes a symbol", () => {
    expect(withoutSymbol(lists, "l1", "BID")[0].symbols).toEqual(["VCB", "CTG"]);
  });

  it("moves a symbol where it is dropped", () => {
    expect(moved(lists, "l1", 2, 0)[0].symbols).toEqual(["CTG", "VCB", "BID"]);
    expect(moved(lists, "l1", 0, 2)[0].symbols).toEqual(["BID", "CTG", "VCB"]);
    expect(moved(lists, "l1", 0, 9)[0]).toBe(banks);
  });

  it("leaves the input lists untouched", () => {
    withSymbol(lists, "l1", "TCB");
    moved(lists, "l1", 0, 1);

    expect(banks.symbols).toEqual(["VCB", "BID", "CTG"]);
  });
});

describe("nextListName", () => {
  it("numbers past the names already taken", () => {
    const nameFor = (n) => `List ${n}`;

    expect(nextListName([], nameFor)).toBe("List 1");
    expect(nextListName([{ id: "a", name: "List 2", symbols: [] }], nameFor)).toBe("List 3");
  });
});

describe("symbolsOf", () => {
  it("shows a named list's own symbols, in its order", () => {
    expect(symbolsOf("l1", lists, ["ACB", "VCB"])).toEqual(["VCB", "BID", "CTG"]);
  });

  it("shows every listed symbol for All, or for a list that was deleted", () => {
    expect(symbolsOf(ALL, lists, ["ACB", "VCB"])).toEqual(["ACB", "VCB"]);
    expect(symbolsOf("gone", lists, ["ACB"])).toEqual(["ACB"]);
  });
});

describe("addProblem", () => {
  const listed = new Set(["VCB", "BID", "CTG", "TCB"]);

  it("accepts a listed symbol not in the list yet, typed any way", () => {
    expect(addProblem(banks, " tcb ", listed)).toBeNull();
  });

  it("says why a symbol cannot join", () => {
    expect(addProblem(banks, "  ", listed)).toBe("empty");
    expect(addProblem(banks, "AAPL", listed)).toBe("notListed");
    expect(addProblem(banks, "vcb", listed)).toBe("already");
  });
});
