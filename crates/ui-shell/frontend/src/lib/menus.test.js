import { beforeEach, describe, expect, it } from "vitest";
import { listMenus, registerMenu, registerMenuItem, resetMenusForTests } from "./menus.js";

beforeEach(() => resetMenusForTests());

describe("registerMenu", () => {
  it("lists menus by their order, not by when they were registered", () => {
    registerMenu({ id: "help", title: "menu.help", order: 90 });
    registerMenu({ id: "file", title: "menu.file", order: 10 });
    registerMenu({ id: "tools", title: "Tools", order: 50 });

    expect(listMenus().map((menu) => menu.id)).toEqual(["file", "tools", "help"]);
  });

  it("refuses a bad id, a second menu with the same id, or no title", () => {
    expect(() => registerMenu({ id: "File", title: "menu.file" })).toThrow(/id/);
    registerMenu({ id: "file", title: "menu.file" });
    expect(() => registerMenu({ id: "file", title: "menu.file" })).toThrow(/already/);
    expect(() => registerMenu({ id: "view" })).toThrow(/title/);
  });
});

describe("registerMenuItem", () => {
  beforeEach(() => registerMenu({ id: "file", title: "menu.file", order: 10 }));

  it("keeps items in groups, groups in the order they were first used", () => {
    registerMenuItem("file", { group: "page", action: "workspace.new-page" });
    registerMenuItem("file", { group: "app", action: "app.exit" });
    registerMenuItem("file", { group: "page", action: "workspace.close-page" });

    const [file] = listMenus();
    expect(file.groups.map((group) => group.map((item) => item.action))).toEqual([
      ["workspace.new-page", "workspace.close-page"],
      ["app.exit"],
    ]);
  });

  it("takes an action, a command, or a list of choices", () => {
    const choices = () => [];
    const run = () => {};
    registerMenuItem("file", { action: "workspace.new-page" });
    registerMenuItem("file", { title: "menu.copy", hint: "Ctrl+C", run });
    registerMenuItem("file", { title: "menu.theme", choices });

    const [items] = listMenus()[0].groups;
    expect(items.map((item) => item.kind)).toEqual(["action", "command", "choices"]);
    expect(items[1]).toMatchObject({ title: "menu.copy", hint: "Ctrl+C", run });
    expect(items[2]).toMatchObject({ title: "menu.theme", choices });
  });

  it("refuses an item that is none of those, or more than one", () => {
    expect(() => registerMenuItem("file", {})).toThrow(/action, a command, or choices/);
    expect(() => registerMenuItem("file", { action: "a.b", title: "x", run: () => {} })).toThrow(
      /action, a command, or choices/,
    );
    expect(() => registerMenuItem("file", { run: () => {} })).toThrow(/title/);
  });

  it("refuses an item for a menu nobody registered", () => {
    expect(() => registerMenuItem("tools", { action: "a.b" })).toThrow(/no menu "tools"/);
  });

  it("does not let a caller change the registered menus through what it lists", () => {
    registerMenuItem("file", { action: "workspace.new-page" });

    listMenus()[0].groups[0].push({ kind: "action", action: "sneaky.item" });

    expect(listMenus()[0].groups[0]).toHaveLength(1);
  });
});
