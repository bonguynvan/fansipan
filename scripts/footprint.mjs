#!/usr/bin/env node
// Measures what the README reports about Fansipan's footprint on Windows:
// install size, start-up time, and memory and processor when idle and under
// a busy feed with three windows open.
//
// From the repository root, after a release build:
//
//   cargo build --release -p fansipan-shell --features tauri/custom-protocol
//   node scripts/footprint.mjs [--runs N]
//
// The app's settings are moved aside for the run, so it starts from the
// default layout, and moved back after, by rename within their own folder.
// If a run is killed before it puts them back, the next run refuses to
// start until `node scripts/footprint.mjs --restore` returns them.
// It refuses to run while any Fansipan is open, adapter examples included,
// since they share the settings. WebView2 gets a profile of its own.

import { execFileSync, spawn } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, renameSync, rmSync, rmdirSync, statSync, writeFileSync } from "node:fs";
import { createServer } from "node:net";
import { cpus, release, tmpdir, totalmem } from "node:os";
import { join } from "node:path";

const EXE = join("target", "release", "fansipan-shell.exe");
const IDENTIFIER = "io.github.bonguynvan.fansipan";
const CONFIG_DIR = join(process.env.APPDATA ?? "", IDENTIFIER);
const BACKUP_DIR = join(CONFIG_DIR, ".footprint-backup");
const SETTINGS_FILES = ["settings.json", ".window-state.json"];
const PORT = 9341;
const STARTUP_TIMEOUT_MS = 60_000;
const CDP_TIMEOUT_MS = 10_000;
const SETTLE_MS = 10_000;
const SAMPLE_MS = 30_000;
const SAMPLE_EVERY_MS = 2_000;
const COLLECTED_SETTLE_MS = 5_000;
/** Cells the price board draws for its sixteen rows: drawn means started. */
const BOARD_CELLS = 16 * 5;
const SCENARIOS = [
  { name: "Idle, one window", beatMs: 500, windows: 1 },
  { name: "Busy feed (10 ms beat), three windows", beatMs: 10, windows: 3 },
];
/** Win32 reports processor time in 100-nanosecond units. */
const TICKS_PER_MS = 10_000;

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const mb = (bytes) => Math.round(bytes / 1024 / 1024);

function powershell(command) {
  return execFileSync("powershell", ["-NoProfile", "-NonInteractive", "-Command", command], { encoding: "utf8" });
}

/** Every shell and WebView2 process, with its memory and processor time. */
function processes() {
  const json = powershell(
    "Get-CimInstance Win32_Process -Filter \"Name='fansipan-shell.exe' OR Name='msedgewebview2.exe'\" | " +
      "Select-Object ProcessId,ParentProcessId,Name,CommandLine,WorkingSetSize,PrivatePageCount,KernelModeTime,UserModeTime | ConvertTo-Json -Compress",
  ).trim();
  if (!json) return [];
  const parsed = JSON.parse(json);
  return Array.isArray(parsed) ? parsed : [parsed];
}

/** `root` and everything it started, however deep. */
function tree(all, root) {
  const ids = new Set([root]);
  let grew = true;
  while (grew) {
    grew = false;
    for (const p of all) {
      if (!ids.has(p.ProcessId) && ids.has(p.ParentProcessId)) {
        ids.add(p.ProcessId);
        grew = true;
      }
    }
  }
  return all.filter((p) => ids.has(p.ProcessId));
}

/** Fansipans other than ours: a shell, or a WebView2 on the app's profile, as an adapter example's is. */
function othersRunning() {
  const all = processes();
  const mine = running ? new Set(tree(all, running.pid).map((p) => p.ProcessId)) : new Set();
  return all.filter(
    (p) =>
      !mine.has(p.ProcessId) &&
      (p.Name === "fansipan-shell.exe" || (p.CommandLine ?? "").toLowerCase().includes(IDENTIFIER)),
  );
}

function portFree(port) {
  return new Promise((resolve) => {
    const server = createServer();
    server.once("error", () => resolve(false));
    server.listen(port, "127.0.0.1", () => server.close(() => resolve(true)));
  });
}

// The trader's settings move aside by rename and back by rename, within
// their own folder, so a file is always whole in one place or the other.
// Each file's move is recorded in the backup folder (the file itself, or a
// marker that it did not exist), so a file never recorded is never touched.

const ABSENT = ".absent";

/** Moves the settings into the backup folder, recording each as it goes. */
function setAside() {
  mkdirSync(BACKUP_DIR);
  for (const name of SETTINGS_FILES) {
    const file = join(CONFIG_DIR, name);
    if (existsSync(file)) renameSync(file, join(BACKUP_DIR, name));
    else writeFileSync(join(BACKUP_DIR, name + ABSENT), "");
  }
}

/** Puts each recorded file back over what the run wrote, then removes the backup folder. */
function restoreSettings() {
  if (!existsSync(BACKUP_DIR)) return;
  for (const name of SETTINGS_FILES) {
    const kept = join(BACKUP_DIR, name);
    const absent = join(BACKUP_DIR, name + ABSENT);
    const file = join(CONFIG_DIR, name);
    if (existsSync(kept)) {
      renameSync(kept, file);
    } else if (existsSync(absent)) {
      rmSync(file, { force: true });
      rmSync(absent);
    }
  }
  rmdirSync(BACKUP_DIR);
}

let running = null;

function stopApp() {
  if (!running) return;
  try {
    execFileSync("taskkill", ["/F", "/T", "/PID", String(running.pid)], { stdio: "ignore" });
  } catch {
    // Already gone.
  }
  running = null;
}

/** Stops the app and returns the settings, unless another Fansipan opened meanwhile. */
function cleanUp() {
  stopApp();
  if (!existsSync(BACKUP_DIR)) return;
  if (othersRunning().length) {
    console.error(
      `footprint: another Fansipan opened during the run, so the settings stay in ${BACKUP_DIR}. ` +
        "Close it and run `node scripts/footprint.mjs --restore`.",
    );
    process.exitCode = 1;
    return;
  }
  try {
    restoreSettings();
  } catch (err) {
    console.error(`footprint: could not put the settings back (${err.message}); they are in ${BACKUP_DIR}.`);
    process.exitCode = 1;
  }
}

for (const signal of ["SIGINT", "SIGTERM", "SIGHUP", "SIGBREAK"]) {
  process.on(signal, () => {
    cleanUp();
    process.exit(130);
  });
}
process.on("uncaughtException", (err) => {
  cleanUp();
  console.error(`footprint: ${err.message}`);
  process.exit(1);
});

// DevTools, one call at a time, each with a timeout.

async function pages() {
  const response = await fetch(`http://127.0.0.1:${PORT}/json/list`, { signal: AbortSignal.timeout(CDP_TIMEOUT_MS) });
  return (await response.json()).filter((p) => p.type === "page");
}

async function call(page, method, params = {}) {
  const socket = new WebSocket(page.webSocketDebuggerUrl);
  try {
    return await new Promise((resolve, reject) => {
      const timer = setTimeout(() => reject(new Error(`${method} timed out`)), CDP_TIMEOUT_MS);
      socket.addEventListener("error", () => reject(new Error(`${method}: the page's socket failed`)));
      socket.addEventListener("close", () => reject(new Error(`${method}: the page closed`)));
      socket.addEventListener("message", (event) => {
        const reply = JSON.parse(event.data);
        if (reply.id !== 1) return;
        clearTimeout(timer);
        if (reply.error) reject(new Error(`${method}: ${reply.error.message}`));
        else resolve(reply.result);
      });
      socket.addEventListener("open", () => socket.send(JSON.stringify({ id: 1, method, params })));
    });
  } finally {
    socket.close();
  }
}

async function evaluate(expression) {
  const [page] = await pages();
  if (!page) return undefined;
  const result = await call(page, "Runtime.evaluate", { expression, awaitPromise: true, returnByValue: true });
  const thrown = result.exceptionDetails;
  if (thrown) throw new Error(thrown.exception?.description ?? thrown.text);
  return result.result?.value;
}

/** Asks every window's JavaScript to collect its garbage now. */
async function collectGarbage() {
  for (const page of await pages()) await call(page, "HeapProfiler.collectGarbage");
}

/** Milliseconds from launch until the price board has drawn its rows. */
async function startUp(started) {
  while (Date.now() - started < STARTUP_TIMEOUT_MS) {
    try {
      const cells = await evaluate("document.querySelectorAll('.board .bo-grid .c').length");
      if (cells >= BOARD_CELLS) return Date.now() - started;
    } catch {
      // The debugging port is not up yet.
    }
    await sleep(25);
  }
  throw new Error("the price board did not draw within a minute");
}

const privateOf = (procs) => procs.reduce((sum, p) => sum + Number(p.PrivatePageCount), 0);

/** Mean memory and processor use of the app's process tree over `ms`. */
async function sample(root, ms) {
  const cpuTime = (procs) => procs.reduce((sum, p) => sum + Number(p.KernelModeTime) + Number(p.UserModeTime), 0);
  const first = tree(processes(), root);
  const begun = Date.now();
  const memory = [];
  let last = first;
  while (Date.now() - begun < ms) {
    await sleep(SAMPLE_EVERY_MS);
    last = tree(processes(), root);
    memory.push({
      working: last.reduce((sum, p) => sum + Number(p.WorkingSetSize), 0),
      private: privateOf(last),
      core: privateOf(last.filter((p) => p.Name === "fansipan-shell.exe")),
    });
  }
  const wall = Date.now() - begun;
  const mean = (key) => memory.reduce((sum, m) => sum + m[key], 0) / memory.length;
  return {
    processes: last.length,
    working: mean("working"),
    private: mean("private"),
    core: mean("core"),
    // A process that starts within the window would count its whole
    // lifetime, and one that ends would drop out; settled, neither happens.
    cpu: ((cpuTime(last) - cpuTime(first)) / TICKS_PER_MS / wall) * 100,
  };
}

async function openWindows(count) {
  for (let n = 1; n < count; n++) {
    // As the app's own pop-out does, the window's layout is stored before
    // it opens (an extra window with none closes itself): here, a copy of
    // the main window's, so every window shows the full default layout.
    const opened = await evaluate(`(async () => {
      const invoke = window.__TAURI_INTERNALS__.invoke;
      let layout = null;
      for (let i = 0; i < 100 && !layout; i++) {
        layout = await invoke("settings_get", { key: "workspace.layout.main" });
        if (!layout) await new Promise((resolve) => setTimeout(resolve, 100));
      }
      if (!layout) return "the main window saved no layout";
      await invoke("settings_set", { key: "workspace.layout.ws-measure${n}", value: layout });
      return invoke("open_window", { label: "ws-measure${n}" }).then(() => "ok", (e) => String(e));
    })()`);
    if (opened !== "ok") throw new Error(`could not open a window: ${opened}`);
  }
}

/** The app's environment: this run's settings, and no other Fansipan or WebView2 ones. */
function appEnv(scenario, profile) {
  const inherited = Object.fromEntries(Object.entries(process.env).filter(([key]) => !/^(FANSIPAN_|WEBVIEW2_)/i.test(key)));
  return {
    ...inherited,
    FANSIPAN_SANDBOX_MARKET: "vn",
    FANSIPAN_SANDBOX_CLOCK: "demo",
    FANSIPAN_SANDBOX_BEAT_MS: String(scenario.beatMs),
    WEBVIEW2_USER_DATA_FOLDER: profile,
    WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${PORT}`,
  };
}

async function measure(scenario, profile) {
  const started = Date.now();
  const child = spawn(EXE, [], { env: appEnv(scenario, profile), stdio: "ignore" });
  running = child;
  const failed = new Promise((_, reject) => {
    child.once("error", (err) => reject(new Error(`could not start ${EXE}: ${err.message}`)));
  });
  failed.catch(() => {});
  try {
    const startUpMs = await Promise.race([startUp(started), failed]);
    await openWindows(scenario.windows);
    await sleep(SETTLE_MS);
    const windows = (await pages()).length;
    if (windows !== scenario.windows) throw new Error(`${scenario.name}: ${windows} windows open, not ${scenario.windows}`);
    const own = tree(processes(), child.pid).filter((p) => p.Name === "msedgewebview2.exe");
    if (!own.some((p) => (p.CommandLine ?? "").includes(profile))) throw new Error("WebView2 did not take the run's own profile");
    const sampled = await sample(child.pid, SAMPLE_MS);
    // Chromium collects lazily where memory is plentiful, so what it holds
    // includes garbage; collected, what is left is what the screens keep.
    await collectGarbage();
    await sleep(COLLECTED_SETTLE_MS);
    const collected = privateOf(tree(processes(), child.pid));
    return { startUpMs, windows, ...sampled, collected };
  } finally {
    stopApp();
    await sleep(1_000);
  }
}

function range(values, format) {
  const [low, high] = [Math.min(...values), Math.max(...values)];
  return format(low) === format(high) ? format(low) : `${format(low)} to ${format(high)}`;
}

function report(results, runs) {
  const cpu = cpus();
  console.log(`Machine: ${cpu[0].model.trim()}, ${cpu.length} logical cores, ${mb(totalmem())} MB RAM, Windows ${release()}`);
  console.log(`Install size: ${mb(statSync(EXE).size)} MB (${EXE}, front end embedded)`);
  console.log(`Runs per scenario: ${runs}; a range is the lowest and highest run.`);
  console.log("");
  console.log(
    "| Scenario | Start-up | Windows | Processes | Private memory | Private, garbage collected | Of which the Rust core | Working set | Processor (share of one core) |",
  );
  console.log("|---|---:|---:|---:|---:|---:|---:|---:|---:|");
  const inMb = (v) => `${mb(v)} MB`;
  for (const scenario of SCENARIOS) {
    const of = (key) => results.filter((r) => r.scenario === scenario).map((r) => r[key]);
    console.log(
      `| ${scenario.name} | ${range(of("startUpMs"), (v) => `${(v / 1000).toFixed(2)} s`)} | ${range(of("windows"), String)} | ` +
        `${range(of("processes"), String)} | ${range(of("private"), inMb)} | ${range(of("collected"), inMb)} | ` +
        `${range(of("core"), inMb)} | ${range(of("working"), inMb)} | ${range(of("cpu"), (v) => `${v.toFixed(0)}%`)} |`,
    );
  }
}

async function main() {
  if (process.platform !== "win32") throw new Error("this harness measures the Windows build");
  if (!process.env.APPDATA) throw new Error("APPDATA is not set");
  if (process.argv.includes("--restore")) {
    if (!existsSync(BACKUP_DIR)) return console.log("footprint: no settings are set aside; nothing to restore.");
    if (othersRunning().length) throw new Error("close every Fansipan first, then restore");
    restoreSettings();
    return console.log("footprint: the settings are back.");
  }
  if (existsSync(BACKUP_DIR)) {
    throw new Error(`a previous run left the settings in ${BACKUP_DIR}; run \`node scripts/footprint.mjs --restore\` first`);
  }
  if (!existsSync(EXE)) throw new Error(`no release build at ${EXE}; see the top of this script`);
  if (othersRunning().length) throw new Error("a Fansipan is open (an adapter example counts: it shares the settings). Close it and run again.");
  if (!(await portFree(PORT))) throw new Error(`port ${PORT}, which the run's debugging port needs, is taken`);
  const at = process.argv.indexOf("--runs");
  const runs = at > 0 ? Number(process.argv[at + 1]) : 1;
  if (!Number.isInteger(runs) || runs < 1 || runs > 10) throw new Error("--runs takes a whole number from 1 to 10");

  const work = mkdtempSync(join(tmpdir(), "fansipan-footprint-"));
  const results = [];
  try {
    setAside();
    for (let run = 0; run < runs; run++) {
      for (const scenario of SCENARIOS) {
        const profile = join(work, `webview-${results.length}`);
        results.push({ scenario, ...(await measure(scenario, profile)) });
        // Each run starts from the defaults again.
        for (const name of SETTINGS_FILES) rmSync(join(CONFIG_DIR, name), { force: true });
      }
    }
  } finally {
    cleanUp();
  }
  report(results, runs);
  try {
    // WebView2 may still hold its profile for a moment after its process ends.
    rmSync(work, { recursive: true, force: true, maxRetries: 10, retryDelay: 500 });
  } catch (err) {
    console.error(`footprint: could not remove ${work} (${err.message}); it holds only the runs' WebView2 profiles.`);
  }
}

main().catch((err) => {
  console.error(`footprint: ${err.message}`);
  process.exitCode = 1;
});
