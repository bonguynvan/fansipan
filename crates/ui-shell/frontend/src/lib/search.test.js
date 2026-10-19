import { describe, expect, it, vi } from "vitest";
import { MAX_RESULTS, commandEntries, fold, matchScore, search, symbolEntries } from "./search.js";

const entry = (label, kind = "command", detail = "") => ({ kind, label, detail, run: () => {} });
const labels = (list) => list.map((e) => e.label);

describe("search", () => {
  it("folds case and accents, Vietnamese đ included", () => {
    expect(fold("Đặt lệnh MUA")).toBe("dat lenh mua");
    expect(fold(undefined)).toBe("");
  });

  it("scores a whole match, then a start, then a word start, then anywhere", () => {
    expect(matchScore("aapl", "aapl")).toBe(0);
    expect(matchScore("new page", "new")).toBe(1);
    expect(matchScore("close page", "page")).toBe(2);
    expect(matchScore("homepage", "page")).toBe(3);
    expect(matchScore("chart", "page")).toBeNull();
    expect(matchScore("chart", "")).toBeNull();
  });

  it("finds nothing for nothing typed", () => {
    expect(search("  ", [entry("New page")])).toEqual([]);
  });

  it("ranks better matches first, a symbol before a command on a tie, then by label", () => {
    const entries = [
      entry("Close page"),
      entry("Homepage"),
      entry("PAGE", "symbol"),
      entry("Page setup"),
      entry("New page"),
    ];

    expect(labels(search("page", entries))).toEqual(["PAGE", "Page setup", "Close page", "New page", "Homepage"]);
  });

  it("matches a detail, below every label match", () => {
    const entries = [entry("AAPL", "symbol", "Apple Inc."), entry("Apply theme")];

    expect(labels(search("apple", entries))).toEqual(["AAPL"]);
    expect(labels(search("app", entries))).toEqual(["Apply theme", "AAPL"]);
  });

  it("finds Vietnamese without its accents", () => {
    expect(labels(search("dat lenh", [entry("Đặt lệnh mua"), entry("Hủy lệnh")]))).toEqual(["Đặt lệnh mua"]);
  });

  it("shows at most a screenful", () => {
    const many = Array.from({ length: 40 }, (_, i) => entry(`Symbol ${i}`, "symbol"));

    expect(search("sym", many)).toHaveLength(MAX_RESULTS);
  });

  it("offers every action with its key, and every menu choice, but not the field commands", () => {
    const runAction = vi.fn();
    const pick = vi.fn();
    const entries = commandEntries({
      actions: [{ id: "workspace.new-page", title: "action.newPage" }],
      menus: [
        {
          groups: [
            [
              { kind: "command", title: "menu.paste", run: () => {} },
              { kind: "choices", title: "menu.theme", choices: () => [{ id: "light", label: "Light", pick }] },
            ],
            [{ kind: "action", action: "workspace.new-page" }],
          ],
        },
      ],
      translate: (id) => `<${id}>`,
      keyOf: (id) => (id === "workspace.new-page" ? "Ctrl+T" : null),
      runAction,
    });

    expect(entries.map(({ label, key }) => [label, key])).toEqual([
      ["<action.newPage>", "Ctrl+T"],
      ["<menu.theme>: Light", undefined],
    ]);
    entries[0].run();
    entries[1].run();
    expect(runAction).toHaveBeenCalledWith("workspace.new-page");
    expect(pick).toHaveBeenCalled();
  });

  it("offers each symbol with its name, picking it when run", () => {
    const pick = vi.fn();
    const [aapl] = symbolEntries(["AAPL"], { AAPL: { name: "Apple Inc." } }, pick);

    aapl.run();

    expect(aapl).toMatchObject({ kind: "symbol", label: "AAPL", detail: "Apple Inc." });
    expect(pick).toHaveBeenCalledWith("AAPL");
  });
});
