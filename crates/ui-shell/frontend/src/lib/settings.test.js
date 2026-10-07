import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const writes = [];
vi.mock("@tauri-apps/api/core", () => ({
  invoke: async (command, args) => {
    writes.push({ command, ...args });
  },
}));

const { debouncedSetter } = await import("./settings.js");

beforeEach(() => {
  writes.length = 0;
  vi.useFakeTimers();
});

afterEach(() => vi.useRealTimers());

describe("debouncedSetter", () => {
  it("writes only the latest value, once, after the delay", async () => {
    const set = debouncedSetter("k", 400);

    set(1);
    set(2);
    set(3);
    expect(writes).toEqual([]);
    await vi.advanceTimersByTimeAsync(400);

    expect(writes).toEqual([{ command: "settings_set", key: "k", value: 3 }]);
  });

  it("flush writes a pending value at once and cancels the timer", async () => {
    const set = debouncedSetter("k", 400);
    set("pending");

    await set.flush();
    await vi.advanceTimersByTimeAsync(1000);

    expect(writes).toEqual([{ command: "settings_set", key: "k", value: "pending" }]);
  });

  it("flush with nothing pending writes nothing", async () => {
    const set = debouncedSetter("k", 400);
    set("once");
    await vi.advanceTimersByTimeAsync(400);

    await set.flush();

    expect(writes).toHaveLength(1);
  });
});
