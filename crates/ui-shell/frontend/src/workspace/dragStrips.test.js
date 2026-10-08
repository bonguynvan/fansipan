import { describe, expect, it } from "vitest";
import { windowHandles } from "./dragStrips.js";

const strip = (id, floating = false) => ({ id, floating });
const handles = (strips) => [...windowHandles(strips, (s) => s.floating)].map((s) => s.id);

describe("windowHandles", () => {
  it("moves the window by the docked strips, leaving floating ones to their group", () => {
    expect(handles([strip("a"), strip("b", true), strip("c")])).toEqual(["a", "c"]);
  });

  it("falls back to the floating strips when nothing is docked", () => {
    expect(handles([strip("a", true), strip("b", true)])).toEqual(["a", "b"]);
  });

  it("has no handle in an empty window", () => {
    expect(handles([])).toEqual([]);
  });
});
