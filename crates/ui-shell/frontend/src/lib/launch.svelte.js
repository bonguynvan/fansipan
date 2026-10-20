// Which venue the terminal runs on. A build that offers several (the
// sandbox's markets, an adapter's accounts) asks at first launch; the shell
// remembers the answer, and changing it restarts the terminal on the new
// one. A build with one venue offers no choice.

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

/** The shell tells every window once a venue answers commands. */
export const STARTED_EVENT = "venue-started";

/** Asked again this many times, a little apart, before giving up. */
const TRIES = 5;
const RETRY_MS = 200;

/**
 * `{ stage: "choosing" | "opening" | "running", choices: [{ id, title }],
 * current, problem }`, as the shell has it.
 */
export const launch = $state({ stage: "opening", choices: [], current: null, problem: null });

const pause = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

/** Reads where the shell stands, asking again briefly if it cannot say yet. */
export async function loadLaunch({ tries = TRIES, retryMs = RETRY_MS } = {}) {
  for (let attempt = 1; ; attempt += 1) {
    try {
      Object.assign(launch, await invoke("launch_state"));
      return launch;
    } catch (err) {
      if (attempt >= tries) throw err;
      await pause(retryMs);
    }
  }
}

/**
 * Resolves once the shell's venue is running: at once if it is, or when
 * the shell says it started. Listens before asking, so a start between the
 * two is not missed.
 */
export async function untilRunning(options) {
  let started;
  const running = new Promise((resolve) => (started = resolve));
  const unlisten = await listen(STARTED_EVENT, (event) => started(event.payload));
  try {
    await loadLaunch(options);
    if (launch.stage === "running") return launch;
    const id = await running;
    Object.assign(launch, { stage: "running", current: id, problem: null });
    return launch;
  } finally {
    unlisten();
  }
}

/** The first choice: the shell connects to it, and the page then loads the workspace. */
export async function chooseVenue(id) {
  await invoke("choose_venue", { id });
  Object.assign(launch, { stage: "running", current: id, problem: null });
}

/** Another venue: the shell remembers it and restarts once every window has saved. */
export function switchVenue(id) {
  if (id === launch.current) return Promise.resolve();
  return invoke("switch_venue", { id });
}
