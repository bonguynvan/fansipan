import { describe, expect, it } from "vitest";
import {
  actionFor,
  bindingsFrom,
  choicesOf,
  comboOf,
  isBindable,
  normalizeCombo,
  rebound,
} from "./hotkeys.js";

const press = (code, key, mods = {}) => ({ code, key, ctrlKey: false, altKey: false, shiftKey: false, metaKey: false, ...mods });

describe("comboOf", () => {
  it("names letters by their place, whatever the layout types", () => {
    expect(comboOf(press("KeyX", "x", { ctrlKey: true, shiftKey: true }))).toBe("Ctrl+Shift+X");
    expect(comboOf(press("KeyQ", "a", { altKey: true }))).toBe("Alt+Q");
  });

  it("names function, digit, and named keys", () => {
    expect(comboOf(press("F1", "F1"))).toBe("F1");
    expect(comboOf(press("Digit5", "%", { shiftKey: true }))).toBe("Shift+5");
    expect(comboOf(press("Escape", "Escape"))).toBe("Escape");
    // The workspace's defaults for walking panels and pages.
    expect(comboOf(press("F6", "F6", { ctrlKey: true, shiftKey: true }))).toBe("Ctrl+Shift+F6");
    expect(comboOf(press("Tab", "Tab", { ctrlKey: true }))).toBe("Ctrl+Tab");
    expect(isBindable("Ctrl+Shift+Tab")).toBe(true);
    expect(comboOf(press("Space", " "))).toBe("Space");
  });

  it("makes nothing of AltGr typing a character", () => {
    expect(comboOf(press("KeyX", "ź", { ctrlKey: true, altKey: true }))).toBeNull();
    expect(comboOf({ ...press("KeyE", "€", { ctrlKey: true, altKey: true }), getModifierState: (m) => m === "AltGraph" })).toBeNull();
    expect(comboOf(press("KeyX", "x", { ctrlKey: true, altKey: true }))).toBe("Ctrl+Alt+X");
  });

  it("makes nothing of a modifier alone or a press mid-composition", () => {
    expect(comboOf(press("ShiftLeft", "Shift", { shiftKey: true }))).toBeNull();
    expect(comboOf({ ...press("KeyA", "a"), isComposing: true })).toBeNull();
    expect(comboOf(press("", "Process"))).toBeNull();
  });
});

describe("normalizeCombo", () => {
  it("writes a combo one way", () => {
    expect(normalizeCombo("shift + ctrl + x")).toBe("Ctrl+Shift+X");
    expect(normalizeCombo("f12")).toBe("F12");
    expect(normalizeCombo("alt+pageup")).toBe("Alt+PageUp");
  });

  it("refuses what is not a combo", () => {
    expect(normalizeCombo("")).toBeNull();
    expect(normalizeCombo("Ctrl+")).toBeNull();
    expect(normalizeCombo("Hyper+X")).toBeNull();
    expect(normalizeCombo("Ctrl+Ctrl+X")).toBeNull();
    expect(normalizeCombo("F25")).toBeNull();
    expect(normalizeCombo(7)).toBeNull();
  });
});

describe("isBindable", () => {
  it("binds function keys and modified combos, never what a field or button needs", () => {
    expect(isBindable("F1")).toBe(true);
    expect(isBindable("Shift+F9")).toBe(true);
    expect(isBindable("Ctrl+Shift+X")).toBe(true);
    expect(isBindable("X")).toBe(false);
    expect(isBindable("Shift+X")).toBe(false);
    expect(isBindable("Enter")).toBe(false);
    expect(isBindable("Space")).toBe(false);
  });
});

const actions = [
  { id: "buy", defaultKey: "F1" },
  { id: "sell", defaultKey: "F2" },
  { id: "cancelAll", defaultKey: "Ctrl+Shift+X" },
];

describe("bindings", () => {
  it("starts from each action's default", () => {
    expect(bindingsFrom(actions, null)).toEqual({ buy: "F1", sell: "F2", cancelAll: "Ctrl+Shift+X" });
  });

  it("takes the trader's choices over the defaults, one action per combo", () => {
    const bindings = bindingsFrom(actions, { sell: "F1", cancelAll: null, gone: "F9", buy: "Nope+Z" });

    expect(bindings).toEqual({ buy: null, sell: "F1", cancelAll: null });
  });

  it("ignores a saved choice no shortcut may use", () => {
    expect(bindingsFrom(actions, { buy: "Enter" }).buy).toBe("F1");
  });

  it("moves a combo to the action it is given to", () => {
    const bindings = rebound(bindingsFrom(actions, null), "cancelAll", "F2");

    expect(bindings).toEqual({ buy: "F1", sell: null, cancelAll: "F2" });
    expect(actionFor(bindings, "F2")).toBe("cancelAll");
    expect(actionFor(bindings, "F9")).toBeNull();
  });

  it("saves only the departures from the defaults", () => {
    const bindings = rebound(bindingsFrom(actions, null), "cancelAll", "F2");

    expect(choicesOf(actions, bindings)).toEqual({ sell: null, cancelAll: "F2" });
    expect(choicesOf(actions, bindingsFrom(actions, null))).toEqual({});
  });
});
