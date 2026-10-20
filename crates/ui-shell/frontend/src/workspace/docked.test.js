import { describe, expect, it } from "vitest";
import { dockedLayout } from "./docked.js";

const leaf = (id, views, activeView = views[0]) => ({ type: "leaf", data: { id, views, activeView }, size: 100 });
const page = (root, extra = {}) => ({
  grid: { root, width: 800, height: 600, orientation: "HORIZONTAL" },
  panels: {},
  activeGroup: "1",
  ...extra,
});
const floating = (id, views) => ({ data: { id, views, activeView: views[0] }, position: { left: 10, top: 10, width: 300, height: 200 } });

describe("dockedLayout", () => {
  it("leaves a page with nothing floating as it is", () => {
    const json = page({ type: "branch", data: [leaf("1", ["board"])], size: 600 });

    expect(dockedLayout(json)).toBe(json);
    expect(dockedLayout(null)).toBeNull();
  });

  it("moves floating and popped-out panels into the dock's first group", () => {
    const json = page(
      { type: "branch", data: [{ type: "branch", data: [leaf("1", ["board"]), leaf("2", ["chart"])], size: 400 }], size: 600 },
      { floatingGroups: [floating("3", ["ticket", "board"])], popoutGroups: [floating("4", ["tape"])], activeGroup: "3" },
    );

    const docked = dockedLayout(json);

    expect(docked.floatingGroups).toEqual([]);
    expect(docked.popoutGroups).toEqual([]);
    const first = docked.grid.root.data[0].data[0].data;
    expect(first.views).toEqual(["board", "ticket", "tape"]);
    expect(first.activeView).toBe("board");
    expect(docked.activeGroup).toBe("1");
    expect(docked.grid.root.data[0].data[1].data.views).toEqual(["chart"]);
    expect(json.floatingGroups).toHaveLength(1);
  });

  it("gives an empty dock a group for what floated", () => {
    const json = page({ type: "branch", data: [], size: 600 }, { floatingGroups: [floating("3", ["ticket"])], activeGroup: "3" });

    const docked = dockedLayout(json);

    expect(docked.grid.root.data).toHaveLength(1);
    expect(docked.grid.root.data[0]).toMatchObject({ size: 800, data: { id: "3", views: ["ticket"], activeView: "ticket" } });
    expect(docked.activeGroup).toBe("3");
  });

  it("sizes a new group across the grid's orientation", () => {
    const json = page({ type: "branch", data: [], size: 800 }, { floatingGroups: [floating("3", ["ticket"])] });
    json.grid.orientation = "VERTICAL";

    expect(dockedLayout(json).grid.root.data[0].size).toBe(600);
  });

  it("keeps floating panels when the save has no grid", () => {
    const json = { panels: {}, floatingGroups: [floating("3", ["ticket"])] };

    const docked = dockedLayout(json);

    expect(docked.grid.root.data[0].data.views).toEqual(["ticket"]);
    expect(docked.floatingGroups).toEqual([]);
  });

  it("drops empty floating groups without touching the dock", () => {
    const json = page({ type: "branch", data: [leaf("1", ["board"])], size: 600 }, { floatingGroups: [floating("3", [])] });

    const docked = dockedLayout(json);

    expect(docked.floatingGroups).toEqual([]);
    expect(docked.grid).toBe(json.grid);
  });
});
