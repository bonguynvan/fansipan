import { beforeEach, describe, expect, it, vi } from "vitest";

const bus = vi.hoisted(() => ({ listeners: [], sent: [], emitError: null, saved: [], stored: undefined, read: null }));

vi.mock("@tauri-apps/api/event", () => ({
  emit: async (event, payload) => {
    if (bus.emitError) throw bus.emitError;
    bus.sent.push({ event, payload });
  },
  listen: async (event, handler) => {
    bus.listeners.push({ event, handler });
    return () => {};
  },
}));
vi.mock("./settings.js", () => ({
  debouncedSetter: () => (value) => bus.saved.push(value),
  getSetting: () => bus.read ?? Promise.resolve(bus.stored),
}));

const { sharedChoice } = await import("./sharedChoice.js");

const CHOICES = ["a", "b", "c"];

function choice() {
  let value = "a";
  const applied = [];
  const shared = sharedChoice({
    setting: "test.choice",
    event: "test-choice",
    isValid: (v) => CHOICES.includes(v),
    current: () => value,
    apply: (v) => {
      value = v;
      applied.push(v);
    },
  });
  return { shared, applied, now: () => value };
}

const deliver = (payload) => bus.listeners.forEach(({ handler }) => handler({ payload }));

beforeEach(() => {
  Object.assign(bus, { listeners: [], sent: [], emitError: null, saved: [], stored: undefined, read: null });
});

describe("sharedChoice", () => {
  it("applies the save at start", async () => {
    bus.stored = "b";
    const { shared, now } = choice();

    await shared.start();

    expect(now()).toBe("b");
  });

  it("applies, saves, and sends a choice", async () => {
    const { shared, now } = choice();
    await shared.start();

    shared.choose("c");

    expect(now()).toBe("c");
    expect(bus.saved).toEqual(["c"]);
    expect(bus.sent.map((s) => s.payload.value)).toEqual(["c"]);
  });

  it("ignores a value that is not a choice, or the one in effect", async () => {
    const { shared } = choice();
    await shared.start();

    shared.choose("z");
    shared.choose("a");

    expect(bus.saved).toEqual([]);
    expect(bus.sent).toEqual([]);
  });

  it("keeps a choice made while the save was still being read", async () => {
    let finish;
    bus.read = new Promise((resolve) => (finish = resolve));
    const { shared, now } = choice();

    const starting = shared.start();
    await vi.waitFor(() => expect(bus.listeners).toHaveLength(1));
    shared.choose("c");
    finish("b");
    await starting;

    expect(now()).toBe("c");
  });

  it("follows another window, but not its own echo or a bad value", async () => {
    const { shared, now } = choice();
    await shared.start();
    shared.choose("b");
    const own = bus.sent[0].payload;

    deliver({ source: "elsewhere", value: "c" });
    expect(now()).toBe("c");
    deliver(own);
    deliver({ source: "elsewhere", value: "z" });

    expect(now()).toBe("c");
  });

  it("reports a choice that could not reach the other windows", async () => {
    const onError = vi.fn();
    const { shared } = choice();
    await shared.start({ onError });
    bus.emitError = new Error("no bus");

    shared.choose("b");

    await vi.waitFor(() => expect(onError).toHaveBeenCalledWith("Error: no bus"));
  });
});
