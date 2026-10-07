import { beforeEach, describe, expect, it } from "vitest";
import {
  getLayout,
  getScreen,
  listLayouts,
  listScreens,
  registerLayout,
  registerScreen,
  resetRegistryForTests,
} from "./registry.js";
import { EXTRA_WINDOW_LABEL, newWindowLabel } from "./labels.js";

const Component = () => {};

beforeEach(() => resetRegistryForTests());

describe("registerScreen", () => {
  it("registers a screen with defaults filled in", () => {
    registerScreen({ id: "price-board", title: "Price board", component: Component, defaultParams: { link: "a" } });

    const screen = getScreen("price-board");
    expect(screen).toMatchObject({ id: "price-board", title: "Price board", linkable: false, description: "" });
    expect(screen.defaultParams).toEqual({ link: "a" });
    expect(listScreens()).toHaveLength(1);
  });

  it("keeps the registered defaults from being changed later", () => {
    const defaults = { link: "a" };
    registerScreen({ id: "chart", title: "Chart", component: Component, defaultParams: defaults });

    defaults.link = "b";

    expect(getScreen("chart").defaultParams.link).toBe("a");
    expect(Object.isFrozen(getScreen("chart").defaultParams)).toBe(true);
  });

  it("refuses ids that would not survive in a saved layout", () => {
    for (const id of ["", "Chart", "1chart", "chart board", "a".repeat(41), undefined]) {
      expect(() => registerScreen({ id, title: "x", component: Component }), String(id)).toThrow(/id/);
    }
  });

  it("refuses a second screen with the same id", () => {
    registerScreen({ id: "chart", title: "Chart", component: Component });

    expect(() => registerScreen({ id: "chart", title: "Other", component: Component })).toThrow(/already registered/);
  });

  it("requires a title and a component", () => {
    expect(() => registerScreen({ id: "a", component: Component })).toThrow(/title/);
    expect(() => registerScreen({ id: "b", title: "B" })).toThrow(/component/);
  });
});

describe("registerLayout", () => {
  it("registers a layout with a build function", () => {
    registerLayout({ id: "scalping", title: "Scalping", build: () => {} });

    expect(getLayout("scalping").title).toBe("Scalping");
    expect(listLayouts().map((l) => l.id)).toEqual(["scalping"]);
  });

  it("refuses a layout without a build function or with a taken id", () => {
    expect(() => registerLayout({ id: "x", title: "X" })).toThrow(/build/);
    registerLayout({ id: "y", title: "Y", build: () => {} });
    expect(() => registerLayout({ id: "y", title: "Y", build: () => {} })).toThrow(/already registered/);
  });
});

describe("newWindowLabel", () => {
  it("produces labels the shell accepts", () => {
    for (const [now, random] of [
      [Date.now(), Math.random()],
      [0, 0.123456],
      [Number.MAX_SAFE_INTEGER, 0.999999],
    ]) {
      expect(newWindowLabel(now, random)).toMatch(EXTRA_WINDOW_LABEL);
    }
  });
});
