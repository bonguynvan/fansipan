import { beforeEach, describe, expect, it, vi } from "vitest";

const calls = [];
let answers = [];
let heard = null;
vi.mock("@tauri-apps/api/core", () => ({
  invoke: async (command, args) => {
    calls.push({ command, ...args });
    if (command !== "launch_state") return null;
    const answer = answers.length > 1 ? answers.shift() : answers[0];
    if (answer instanceof Error) throw answer;
    return answer;
  },
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: async (event, handler) => {
    heard = { event, handler };
    return () => (heard = null);
  },
}));

const { STARTED_EVENT, chooseVenue, launch, loadLaunch, switchVenue, untilRunning } = await import(
  "./launch.svelte.js"
);

const choices = [
  { id: "us", title: "United States" },
  { id: "vn", title: "Vietnam" },
];
const at = (stage, current = null, problem = null) => ({ stage, choices, current, problem });
const quick = { retryMs: 0 };

beforeEach(() => {
  calls.length = 0;
  answers = [];
  heard = null;
});

describe("launch", () => {
  it("takes the shell's stage and choices", async () => {
    answers = [at("choosing", null, "no route")];

    await loadLaunch(quick);

    expect(launch).toMatchObject({ stage: "choosing", problem: "no route" });
    expect(launch.choices.map((c) => c.id)).toEqual(["us", "vn"]);
  });

  it("asks again while the shell cannot say, then gives up", async () => {
    answers = [new Error("not ready"), at("running", "us")];
    await loadLaunch(quick);
    expect(launch.current).toBe("us");

    answers = [new Error("broken")];
    await expect(loadLaunch({ tries: 3, retryMs: 0 })).rejects.toThrow("broken");
    expect(calls.filter((c) => c.command === "launch_state")).toHaveLength(2 + 3);
  });

  it("waits for the shell to say the venue started", async () => {
    answers = [at("opening")];

    const waiting = untilRunning(quick);
    await vi.waitFor(() => expect(calls.length).toBe(1));
    expect(heard.event).toBe(STARTED_EVENT);
    heard.handler({ payload: "vn" });

    await expect(waiting).resolves.toMatchObject({ stage: "running", current: "vn" });
    expect(heard).toBeNull();
  });

  it("does not wait when the venue already runs", async () => {
    answers = [at("running", "us")];

    await expect(untilRunning(quick)).resolves.toMatchObject({ current: "us" });
    expect(heard).toBeNull();
  });

  it("runs the first choice", async () => {
    await chooseVenue("vn");

    expect(calls.at(-1)).toEqual({ command: "choose_venue", id: "vn" });
    expect(launch).toMatchObject({ stage: "running", current: "vn", problem: null });
  });

  it("asks the shell to change only to another venue", async () => {
    answers = [at("running", "us")];
    await loadLaunch(quick);
    calls.length = 0;

    await switchVenue("us");
    expect(calls).toEqual([]);

    await switchVenue("vn");
    expect(calls).toEqual([{ command: "switch_venue", id: "vn" }]);
  });
});
