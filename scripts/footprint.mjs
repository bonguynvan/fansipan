#!/usr/bin/env node
// Measures what the README reports about Fansipan's footprint on Windows:
// install size, start-up time, and memory and CPU when idle and under a busy
// feed with three windows open (docs/performance.md, "Footprint").
//
// From the repository root, after a release build:
//
//   cargo build --release -p fansipan-shell --features tauri/custom-protocol
//   node scripts/footprint.mjs
//
// It refuses to run while any Fansipan is open, adapter examples included,
// since they share its settings. For the run it sets the app's settings
// aside, so it starts from the default layout, and puts them back after.
// WebView2 gets a profile of its own in a temporary folder.

import { execFileSync, spawn } from "node:child_process";
import { copyFileSync, existsSync, mkdirSync, mkdtempSync, rmSync, statSync } from "node:fs";
import { cpus, release, tmpdir, totalmem } from "node:os";
import { join } from "node:path";

const EXE = join("target", "release", "fansipan-shell.exe");
const IDENTIFIER = "io.github.bonguynvan.fansipan";
const SETTINGS_FILES = ["settings.json", ".window-state.json"];
const PORT = 9341;
const STARTUP_TIMEOUT_MS = 60_000;
const SETTLE_MS = 10_000;
const SAMPLE_MS = 30_000;
const SAMPLE_EVERY_MS = 2_000;
/** Cells the price board draws for its sixteen rows: drawn means started. */
const BOARD_CELLS = 16 * 5;
const SCENARIOS = [
  { name: "Idle, one window", beatMs: 500, windows: 1 },
  { name: "Busy feed (10 ms beat), three windows", beatMs: 10, windows: 3 },
];
/** Win32 reports processor time in 100-nanosecond units. */
const TICKS_PER_MS = 10_000;

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const mb = (bytes) => (bytes / 1024 / 1024).toFixed(0);

function powershell(command) {
  return execFileSync("powershell", ["-NoProfile", "-NonInteractive", "-Command", command], { encoding: "utf8" });
}

/** Every Fansipan shell and WebView2 process, with its memory and processor time. */
function processes() {
  const json = powershell(
    "Get-CimInstance Win32_Process -Filter \"Name='fansipan-shell.exe' OR Name='msedgewebview2.exe' OR Name='terminal.exe'\" | " +
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

function refuseIfRunning() {
  const open = processes().filter(
    (p) => p.Name === "fansipan-shell.exe" || (p.Name === "msedgewebview2.exe" && (p.CommandLine ?? "").includes("fansipan")),
  );
  if (open.length) {
    throw new Error(
      "A Fansipan is open (an adapter example counts: it shares the settings). Close it and run again.",
    );
  }
}

/** Moves the app's settings into `aside`, so the run starts from the defaults. */
function setAside(aside) {
  const dir = join(process.env.APPDATA, IDENTIFIER);
  for (const name of SETTINGS_FILES) {
    const file = join(dir, name);
    if (existsSync(file)) {
      copyFileSync(file, join(aside, name));
      rmSync(file);
    }
  }
  return () => {
    for (const name of SETTINGS_FILES) {
      const file = join(dir, name);
      rmSync(file, { force: true });
      if (existsSync(join(aside, name))) {
        mkdirSync(dir, { recursive: true });
        copyFileSync(join(aside, name), file);
      }
    }
  };
}

async function evaluate(expression) {
  const pages = await (await fetch(`http://127.0.0.1:${PORT}/json/list`)).json();
  const page = pages.find((p) => p.type === "page");
  if (!page) return undefined;
  const socket = new WebSocket(page.webSocketDebuggerUrl);
  await new Promise((resolve, reject) => {
    socket.addEventListener("open", resolve);
    socket.addEventListener("error", reject);
  });
  const reply = await new Promise((resolve) => {
    socket.addEventListener("message", (event) => resolve(JSON.parse(event.data)));
    socket.send(JSON.stringify({ id: 1, method: "Runtime.evaluate", params: { expression, awaitPromise: true, returnByValue: true } }));
  });
  socket.close();
  return reply.result?.result?.value;
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

/** Mean memory and processor use of the shell's process tree over `ms`. */
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
      private: last.reduce((sum, p) => sum + Number(p.PrivatePageCount), 0),
    });
  }
  const wall = Date.now() - begun;
  const mean = (key) => memory.reduce((sum, m) => sum + m[key], 0) / memory.length;
  return {
    processes: last.length,
    working: mean("working"),
    private: mean("private"),
    // Processes that ended in the window drop out of the sum; none should.
    cpuOfOneCore: ((cpuTime(last) - cpuTime(first)) / TICKS_PER_MS / wall) * 100,
  };
}

async function measure(scenario, profile) {
  const started = Date.now();
  const child = spawn(EXE, [], {
    env: {
      ...process.env,
      FANSIPAN_SANDBOX_MARKET: "vn",
      FANSIPAN_SANDBOX_BEAT_MS: String(scenario.beatMs),
      WEBVIEW2_USER_DATA_FOLDER: profile,
      WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${PORT}`,
    },
    stdio: "ignore",
  });
  try {
    const startUpMs = await startUp(started);
    for (let n = 1; n < scenario.windows; n++) {
      await evaluate(`window.__TAURI_INTERNALS__.invoke("open_window", { label: "ws-measure${n}" })`);
    }
    await sleep(SETTLE_MS);
    return { startUpMs, ...(await sample(child.pid, SAMPLE_MS)) };
  } finally {
    execFileSync("taskkill", ["/F", "/T", "/PID", String(child.pid)], { stdio: "ignore" });
    await sleep(1_000);
  }
}

async function main() {
  if (process.platform !== "win32") throw new Error("this harness measures the Windows build");
  if (!existsSync(EXE)) throw new Error(`no release build at ${EXE}; see the top of this script`);
  refuseIfRunning();

  const work = mkdtempSync(join(tmpdir(), "fansipan-footprint-"));
  const aside = join(work, "settings");
  mkdirSync(aside);
  const restore = setAside(aside);
  const results = [];
  try {
    for (const scenario of SCENARIOS) {
      const profile = join(work, `webview-${results.length}`);
      results.push({ scenario, ...(await measure(scenario, profile)) });
      // Each run starts from the defaults again.
      for (const name of SETTINGS_FILES) rmSync(join(process.env.APPDATA, IDENTIFIER, name), { force: true });
    }
  } finally {
    restore();
    rmSync(work, { recursive: true, force: true });
  }

  const cpu = cpus();
  console.log(`Machine: ${cpu[0].model.trim()}, ${cpu.length} logical cores, ${mb(totalmem())} MB RAM, Windows ${release()}`);
  console.log(`Install size: ${mb(statSync(EXE).size)} MB (${EXE}, front end embedded)`);
  console.log("");
  console.log("| Scenario | Start-up | Processes | Working set | Private | CPU (of one core) |");
  console.log("|---|---:|---:|---:|---:|---:|");
  for (const r of results) {
    console.log(
      `| ${r.scenario.name} | ${(r.startUpMs / 1000).toFixed(2)} s | ${r.processes} | ${mb(r.working)} MB | ${mb(r.private)} MB | ${r.cpuOfOneCore.toFixed(1)}% |`,
    );
  }
}

main().catch((err) => {
  console.error(`footprint: ${err.message}`);
  process.exitCode = 1;
});
