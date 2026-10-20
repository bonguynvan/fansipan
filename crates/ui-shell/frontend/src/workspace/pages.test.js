import { describe, expect, it } from "vitest";
import {
  VERSION,
  activeDesktop,
  activePage,
  addDesktop,
  addIncoming,
  addPage,
  closePage,
  deletePage,
  fromSingleLayout,
  movePanelToPage,
  openPages,
  panelsOf,
  removeDesktop,
  renameDesktop,
  renamePage,
  reopenPage,
  reviveWorkspace,
  savedPages,
  seedWorkspace,
  switchDesktop,
  switchPage,
  takeIncoming,
  uniqueName,
  withPageLayout,
} from "./pages.js";

const names = { desktop: (n) => `Desktop ${n}`, page: (n) => `Page ${n}` };
const layout = (label) => ({ grid: { label }, panels: {} });
const panel = (component) => ({ component, params: { linked: true }, title: null });

describe("pages", () => {
  it("starts with one desktop and one open, empty page", () => {
    const ws = seedWorkspace(names);

    expect(ws.desktops).toHaveLength(1);
    expect(activeDesktop(ws).name).toBe("Desktop 1");
    expect(activePage(ws)).toMatchObject({ name: "Page 1", open: true, dockview: null, incoming: [] });
  });

  it("adds pages with unique names and switches between them", () => {
    let ws = seedWorkspace(names, { pageName: "Board" });
    ws = addPage(ws, "Board");
    const second = activePage(ws);

    expect(second.name).toBe("Board 2");
    ws = switchPage(ws, openPages(ws)[0].id);
    expect(activePage(ws).name).toBe("Board");
    expect(switchPage(ws, "nowhere")).toBe(ws);
  });

  it("keeps the arrangement of the page on screen", () => {
    const ws = withPageLayout(seedWorkspace(names), layout("trading"));

    expect(activePage(ws).dockview).toEqual(layout("trading"));
  });

  it("copies an arrangement into a new page", () => {
    const ws = addPage(seedWorkspace(names), "Copy", layout("trading"));

    expect(activePage(ws).dockview).toEqual(layout("trading"));
    expect(openPages(ws)).toHaveLength(2);
  });

  it("closes a tab into the saved list and reopens it", () => {
    let ws = addPage(seedWorkspace(names), "Board");
    const board = activePage(ws).id;

    ws = closePage(ws, board, names);
    expect(openPages(ws).map((p) => p.name)).toEqual(["Page 1"]);
    expect(savedPages(ws).map((p) => p.name)).toEqual(["Board"]);
    expect(activePage(ws).name).toBe("Page 1");

    ws = reopenPage(ws, board);
    expect(activePage(ws).id).toBe(board);
    expect(savedPages(ws)).toEqual([]);
  });

  it("always keeps an open page", () => {
    const ws = seedWorkspace(names);
    const only = activePage(ws).id;

    const closed = closePage(ws, only, names);
    expect(openPages(closed)).toHaveLength(1);
    expect(activePage(closed).id).not.toBe(only);

    const deleted = deletePage(ws, only, names);
    expect(activeDesktop(deleted).pages).toHaveLength(1);
    expect(activePage(deleted).open).toBe(true);
  });

  it("renames, but not to nothing", () => {
    const ws = seedWorkspace(names);
    const id = activePage(ws).id;

    expect(activePage(renamePage(ws, id, "  Morning  ")).name).toBe("Morning");
    expect(renamePage(ws, id, "  ")).toBe(ws);
  });

  it("moves a panel to another page, shown there next time", () => {
    let ws = addPage(seedWorkspace(names), "Board");
    const board = activePage(ws).id;
    ws = switchPage(ws, openPages(ws)[0].id);

    ws = movePanelToPage(ws, board, panel("chart"));
    expect(movePanelToPage(ws, activePage(ws).id, panel("chart"))).toBe(ws);

    ws = switchPage(ws, board);
    const [after, waiting] = takeIncoming(ws);
    expect(waiting).toEqual([panel("chart")]);
    expect(activePage(after).incoming).toEqual([]);
    expect(takeIncoming(after)[1]).toEqual([]);
  });

  it("never changes the workspace it was given", () => {
    const ws = seedWorkspace(names);
    const before = JSON.stringify(ws);

    addPage(ws, "Board");
    closePage(ws, activePage(ws).id, names);
    withPageLayout(ws, layout("x"));

    expect(JSON.stringify(ws)).toBe(before);
  });

  it("names pages uniquely", () => {
    expect(uniqueName("Board", ["Board", "Board 2"])).toBe("Board 3");
    expect(uniqueName("Board", [])).toBe("Board");
  });
});

describe("desktops", () => {
  it("adds, renames, switches, and removes desktops, keeping the last", () => {
    let ws = addDesktop(seedWorkspace(names), names);
    const second = activeDesktop(ws).id;

    expect(activeDesktop(ws).name).toBe("Desktop 2");
    expect(openPages(ws).map((p) => p.name)).toEqual(["Page 1"]);

    ws = renameDesktop(ws, second, "Evening");
    expect(activeDesktop(ws).name).toBe("Evening");

    ws = switchDesktop(ws, ws.desktops[0].id);
    expect(activeDesktop(ws).name).toBe("Desktop 1");

    ws = removeDesktop(ws, ws.desktops[0].id);
    expect(ws.desktops.map((d) => d.name)).toEqual(["Evening"]);
    expect(removeDesktop(ws, second)).toBe(ws);
  });
});

describe("reading back a save", () => {
  it("restores what it saved", () => {
    const ws = withPageLayout(addPage(seedWorkspace(names), "Board"), layout("board"));

    expect(reviveWorkspace(JSON.parse(JSON.stringify(ws)), names)).toEqual(ws);
  });

  it("repairs a damaged save instead of failing", () => {
    const saved = {
      version: VERSION,
      desktops: [
        {
          id: "d",
          name: "",
          activePageId: "gone",
          pages: [
            { id: "p", name: "One", open: false, dockview: "nonsense", incoming: [{ component: "chart" }, { nope: 1 }] },
            { id: "p", name: "Duplicate id", open: false },
          ],
        },
        "not a desktop",
      ],
      activeDesktopId: "gone",
    };

    const ws = reviveWorkspace(saved, names);

    expect(ws.desktops).toHaveLength(1);
    expect(activeDesktop(ws).name).toBe("Desktop 1");
    const pages = activeDesktop(ws).pages;
    expect(new Set(pages.map((p) => p.id)).size).toBe(pages.length);
    expect(pages[0]).toMatchObject({ dockview: null, incoming: [{ component: "chart", params: {}, title: null }] });
    expect(activePage(ws).open).toBe(true);
  });

  it("reads nothing else as a workspace", () => {
    expect(reviveWorkspace(null, names)).toBeNull();
    expect(reviveWorkspace({ version: 1, dockview: {} }, names)).toBeNull();
    expect(reviveWorkspace({ version: VERSION, desktops: [] }, names)).toBeNull();
  });

  it("makes a single layout from before pages the first page", () => {
    const ws = fromSingleLayout({ version: 1, dockview: layout("old") }, names, "Trading desk");

    expect(activePage(ws)).toMatchObject({ name: "Trading desk", dockview: layout("old") });
  });

  it("makes a pop-out's panel list the first page's panels", () => {
    const ws = fromSingleLayout({ version: 1, seed: [{ component: "chart", params: { linked: true } }] }, names, "Page");

    expect(activePage(ws).incoming).toEqual([{ component: "chart", params: { linked: true }, title: null }]);
    expect(fromSingleLayout({ version: 1, seed: [] }, names, "Page")).toBeNull();
    expect(fromSingleLayout({ version: 3 }, names, "Page")).toBeNull();
  });
});

describe("panels from elsewhere", () => {
  const chart = { component: "chart", params: { linked: true }, title: null };

  it("lists every panel of every page and desktop, waiting ones included", () => {
    let ws = withPageLayout(seedWorkspace(names), {
      panels: {
        "board-1": { id: "board-1", contentComponent: "board", params: { titleKey: "screen.board" } },
        broken: { id: "broken" },
      },
    });
    ws = addDesktop(ws, names);
    ws = addIncoming(ws, [chart]);

    expect(panelsOf(ws)).toEqual([{ component: "board", params: { titleKey: "screen.board" }, title: "screen.board" }, chart]);
  });

  it("puts panels on the page on screen, to show when it is next shown", () => {
    const ws = addIncoming(seedWorkspace(names), [chart, chart]);

    const [rest, incoming] = takeIncoming(ws);

    expect(incoming).toEqual([chart, chart]);
    expect(activePage(rest).incoming).toEqual([]);
    expect(addIncoming(rest, [])).toBe(rest);
  });
});
