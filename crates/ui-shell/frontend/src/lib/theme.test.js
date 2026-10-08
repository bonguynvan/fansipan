import { describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/event", () => ({ emit: async () => {}, listen: async () => () => {} }));
vi.mock("./settings.js", () => ({ debouncedSetter: () => () => {}, getSetting: async () => null }));

const { THEMES, isTheme, resolveTheme } = await import("./theme.svelte.js");

describe("themes", () => {
  it("offers dark, light, and the system's own", () => {
    expect(THEMES).toEqual(["dark", "light", "system"]);
    expect(isTheme("system")).toBe(true);
    expect(isTheme("sepia")).toBe(false);
    expect(isTheme(undefined)).toBe(false);
  });

  it("shows a chosen theme whatever the system prefers", () => {
    expect(resolveTheme("dark", true)).toBe("dark");
    expect(resolveTheme("light", false)).toBe("light");
  });

  it("follows the system when the trader chose it", () => {
    expect(resolveTheme("system", true)).toBe("light");
    expect(resolveTheme("system", false)).toBe("dark");
  });
});
