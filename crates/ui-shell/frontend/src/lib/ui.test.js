import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/event", () => ({ emit: async () => {}, listen: async () => () => {} }));
vi.mock("./settings.js", () => ({ debouncedSetter: () => () => {}, getSetting: async () => null }));

const { BASE_ROOT_PX, SCALES, isScale, px, remToPx, ui } = await import("./ui.svelte.js");
const { WIDTHS, sizeMarks } = await import("./panelSize.svelte.js");

afterEach(() => {
  ui.rootPx = BASE_ROOT_PX;
});

describe("px", () => {
  it("is the designed size at 100%", () => {
    expect(px(36)).toBe(36);
    expect(remToPx(2)).toBe(32);
  });

  it("follows the root font size", () => {
    ui.rootPx = BASE_ROOT_PX * 1.3;

    expect(px(36)).toBe(47);
    expect(remToPx(2)).toBeCloseTo(41.6);
  });

  it("offers only the listed text sizes", () => {
    expect(SCALES).toContain(1);
    expect(isScale(1.15)).toBe(true);
    expect(isScale(2)).toBe(false);
    expect(isScale("1")).toBe(false);
  });
});

describe("sizeMarks", () => {
  const at = (rem) => sizeMarks(rem * 16, 16);

  it("marks a screen by its width in rem", () => {
    expect(at(18)).toEqual({ narrow: true, compact: true, wide: false });
    expect(at(25)).toEqual({ narrow: false, compact: true, wide: false });
    expect(at(40)).toEqual({ narrow: false, compact: false, wide: false });
    expect(at(WIDTHS.wide)).toEqual({ narrow: false, compact: false, wide: true });
  });

  it("moves with the text size: the same panel is narrower in rem at 130%", () => {
    expect(sizeMarks(400, 16).compact).toBe(true);
    expect(sizeMarks(500, 16).compact).toBe(false);
    expect(sizeMarks(500, 16 * 1.3).compact).toBe(true);
  });
});
