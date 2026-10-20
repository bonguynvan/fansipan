#!/usr/bin/env node
// Measures the Docker demo's footprint, as scripts/footprint.mjs does on
// Windows: the image's size,
// start-up, and memory and processor when idle and under a busy feed with
// three windows open.
//
// From the repository root, after building the image:
//
//   docker compose build
//   node scripts/footprint-docker.mjs [--runs N]
//
// Each run starts its own container on a fresh volume, so it touches
// neither the demo's settings nor a demo already running. WebKitGTK offers
// no debugging port to watch the board draw, so start-up runs to the
// terminal's windows appearing on the container's virtual display.

import { execFileSync, spawnSync } from "node:child_process";
import { randomBytes } from "node:crypto";

const IMAGE = "fansipan:local";
const CONFIG = "/home/fansipan/.config";
const SETTINGS = `${CONFIG}/io.github.bonguynvan.fansipan/settings.json`;
const SETTLE_MS = 10_000;
const SAMPLE_MS = 30_000;
const SAMPLE_EVERY_MS = 3_000;
const STARTUP_TIMEOUT_S = 120;
const SCENARIOS = [
  { name: "Idle, one window", beatMs: 500, windows: 1 },
  { name: "Busy feed (10 ms beat), three windows", beatMs: 10, windows: 3 },
];
/** What each process in the container is, by its command name. */
const GROUPS = [
  { group: "shell", match: /^fansipan$/ },
  { group: "webkit", match: /^WebKit/ },
  { group: "display", match: /^(Xvfb|x11vnc|openbox|websockify|python3|dbus-daemon|dbus-run-sessio)/ },
];

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const mb = (kb) => Math.round(kb / 1024);

function docker(args, options = {}) {
  return execFileSync("docker", args, { encoding: "utf8", stdio: ["pipe", "pipe", "pipe"], ...options }).trim();
}

function groupOf(comm) {
  return GROUPS.find(({ match }) => match.test(comm))?.group ?? "other";
}

// One line a process: pid, command, processor ticks, and its own memory
// in kB: its share of anonymous and shared memory (Pss_Anon + Pss_Shmem),
// not of the files it maps, which the container's memory below leaves out
// too; then the container's own memory and processor.
const PROBE = `
for d in /proc/[0-9]*; do
  [ -r "$d/smaps_rollup" ] || continue
  comm=$(cat "$d/comm" 2>/dev/null) || continue
  ticks=$(awk '{print $14 + $15}' "$d/stat" 2>/dev/null) || continue
  pss=$(awk '/^Pss_Anon:|^Pss_Shmem:/ {s += $2} END {print s + 0}' "$d/smaps_rollup" 2>/dev/null)
  rss=$(awk '/^Rss:/ {print $2}' "$d/smaps_rollup" 2>/dev/null)
  echo "p \${d#/proc/} $comm|$ticks $pss $rss"
done
echo "m $(cat /sys/fs/cgroup/memory.current) $(awk '/^inactive_file / {print $2}' /sys/fs/cgroup/memory.stat)"
echo "c $(awk '/^usage_usec / {print $2}' /sys/fs/cgroup/cpu.stat)"
echo "t $(getconf CLK_TCK) $(date +%s%N)"
`;

function probe(container) {
  const out = docker(["exec", container, "sh", "-c", PROBE]);
  const processes = [];
  let memory = 0;
  let cpuUsec = 0;
  let tick = 100;
  let nanos = 0;
  for (const line of out.split("\n")) {
    const [kind, ...rest] = line.split(" ");
    if (kind === "p") {
      const [pid, tail] = [rest[0], rest.slice(1).join(" ")];
      const [comm, numbers] = tail.split("|");
      const [ticks, pss, rss] = numbers.split(" ").map(Number);
      processes.push({ pid, comm, group: groupOf(comm), ticks, pss: pss || 0, rss: rss || 0 });
    } else if (kind === "m") {
      memory = (Number(rest[0]) - Number(rest[1] || 0)) / 1024;
    } else if (kind === "c") {
      cpuUsec = Number(rest[0]);
    } else if (kind === "t") {
      tick = Number(rest[0]);
      nanos = Number(rest[1]);
    }
  }
  return { processes, memoryKb: memory, cpuUsec, tick, nanos };
}

/** The terminal's windows on the virtual display, by their title. */
function windowsShown(container) {
  const tree = spawnSync("docker", ["exec", container, "xwininfo", "-display", ":99", "-root", "-tree"], {
    encoding: "utf8",
  });
  return (tree.stdout.match(/"Fansipan"/g) ?? []).length;
}

function startedAtNanos(container) {
  const iso = docker(["inspect", "-f", "{{.State.StartedAt}}", container]);
  const [whole, fraction = ""] = iso.replace("Z", "").split(".");
  return BigInt(Date.parse(`${whole}Z`)) * 1_000_000n + BigInt(fraction.padEnd(9, "0").slice(0, 9));
}

function run(name, volume, scenario) {
  docker([
    "run", "-d", "--name", name, "--init", "--shm-size", "512m",
    "-e", "FANSIPAN_SANDBOX_MARKET=vn",
    "-e", "FANSIPAN_SANDBOX_CLOCK=demo",
    "-e", `FANSIPAN_SANDBOX_BEAT_MS=${scenario.beatMs}`,
    "-v", `${volume}:${CONFIG}`,
    IMAGE,
  ]);
}

function remove(name) {
  spawnSync("docker", ["rm", "-f", name], { stdio: "ignore" });
}

/** Waits for `count` windows, from inside the container; returns when, in ns since the epoch. */
function waitForWindows(container, count) {
  const script =
    `i=0; until [ "$(xwininfo -display :99 -root -tree 2>/dev/null | grep -c '"Fansipan"')" -ge ${count} ]; do ` +
    `i=$((i+1)); [ $i -gt ${STARTUP_TIMEOUT_S * 20} ] && exit 1; sleep 0.05; done; date +%s%N`;
  const out = spawnSync("docker", ["exec", container, "sh", "-c", script], { encoding: "utf8" });
  if (out.status !== 0) throw new Error(`${count} windows did not open within ${STARTUP_TIMEOUT_S} s`);
  return BigInt(out.stdout.trim());
}

/** Reads the volume's settings, through the image itself. */
function readSettings(volume) {
  const out = spawnSync(
    "docker",
    ["run", "--rm", "-v", `${volume}:${CONFIG}`, "--entrypoint", "cat", IMAGE, SETTINGS],
    { encoding: "utf8" },
  );
  return out.status === 0 ? JSON.parse(out.stdout) : null;
}

function writeSettings(volume, settings) {
  const out = spawnSync(
    "docker",
    ["run", "--rm", "-i", "-v", `${volume}:${CONFIG}`, "--entrypoint", "sh", IMAGE, "-c", `cat > ${SETTINGS}`],
    { input: JSON.stringify(settings, null, 2), encoding: "utf8" },
  );
  if (out.status !== 0) throw new Error(`could not write the settings: ${out.stderr}`);
}

/**
 * Seeds `volume` so the terminal reopens `count - 1` extra windows, each
 * with the main window's default layout, as a pop-out stores one: starts
 * once to let the main window save its layout, then lists the windows.
 */
async function seedWindows(volume, count, scenario) {
  const name = `fansipan-seed-${randomBytes(3).toString("hex")}`;
  run(name, volume, scenario);
  try {
    waitForWindows(name, 1);
    let layout = null;
    for (let i = 0; i < 60 && !layout; i++) {
      await sleep(500);
      const cat = spawnSync("docker", ["exec", name, "cat", SETTINGS], { encoding: "utf8" });
      if (cat.status === 0) layout = JSON.parse(cat.stdout)["workspace.layout.main"] ?? null;
    }
    if (!layout) throw new Error("the main window saved no layout");
  } finally {
    docker(["stop", "-t", "10", name]);
    remove(name);
  }
  const settings = readSettings(volume) ?? {};
  const extra = Array.from({ length: count - 1 }, (_, i) => `ws-measure${i + 2}`);
  settings["workspace.windows"] = extra;
  for (const label of extra) settings[`workspace.layout.${label}`] = settings["workspace.layout.main"];
  writeSettings(volume, settings);
}

/** Averages memory over the samples, and processor time between the first and the last. */
function summarize(samples) {
  const byGroup = (sample, key) => {
    const sums = {};
    for (const p of sample.processes) sums[p.group] = (sums[p.group] ?? 0) + p[key];
    return sums;
  };
  const average = (values) => values.reduce((a, b) => a + b, 0) / values.length;
  const groups = ["shell", "webkit", "display", "other"];
  const pss = Object.fromEntries(groups.map((g) => [g, average(samples.map((s) => byGroup(s, "pss")[g] ?? 0))]));
  const first = samples[0];
  const last = samples[samples.length - 1];
  const seconds = Number(BigInt(last.nanos) - BigInt(first.nanos)) / 1e9;
  const ticksOf = (sample) => new Map(sample.processes.map((p) => [p.pid, p]));
  const before = ticksOf(first);
  const cpu = Object.fromEntries(groups.map((g) => [g, 0]));
  for (const p of last.processes) {
    const was = before.get(p.pid);
    cpu[p.group] += (p.ticks - (was?.ticks ?? 0)) / last.tick / seconds;
  }
  return {
    processes: last.processes.length,
    containerMb: mb(average(samples.map((s) => s.memoryKb))),
    pssMb: Object.fromEntries(groups.map((g) => [g, mb(pss[g])])),
    terminalMb: mb(pss.shell + pss.webkit),
    containerCpu: (last.cpuUsec - first.cpuUsec) / 1e6 / seconds,
    cpu,
  };
}

async function measure(scenario) {
  const volume = `fansipan-footprint-${randomBytes(3).toString("hex")}`;
  const name = `fansipan-footprint-${randomBytes(3).toString("hex")}`;
  docker(["volume", "create", volume]);
  try {
    if (scenario.windows > 1) await seedWindows(volume, scenario.windows, scenario);
    run(name, volume, scenario);
    const shown = waitForWindows(name, scenario.windows);
    const startUpMs = Number(shown - startedAtNanos(name)) / 1e6;
    await sleep(SETTLE_MS);
    if (windowsShown(name) < scenario.windows) throw new Error(`${scenario.name}: fewer than ${scenario.windows} windows`);
    const samples = [];
    const end = Date.now() + SAMPLE_MS;
    while (Date.now() < end) {
      samples.push(probe(name));
      await sleep(SAMPLE_EVERY_MS);
    }
    samples.push(probe(name));
    return { startUpMs, windows: windowsShown(name), ...summarize(samples) };
  } finally {
    remove(name);
    spawnSync("docker", ["volume", "rm", "-f", volume], { stdio: "ignore" });
  }
}

const range = (values, format) => {
  const low = Math.min(...values);
  const high = Math.max(...values);
  return format(low) === format(high) ? format(low) : `${format(low)} to ${format(high)}`;
};

function report(results, runs) {
  const size = Number(docker(["image", "inspect", "-f", "{{.Size}}", IMAGE]));
  const info = JSON.parse(docker(["info", "--format", "{{json .}}"]));
  console.log(
    `\nDocker ${info.ServerVersion} on ${info.OperatingSystem}, ${info.NCPU} CPUs and ` +
      `${Math.round(info.MemTotal / 1024 ** 3)} GB for containers; ${runs} run(s) of each load.\n`,
  );
  const onDisk = docker(["image", "ls", IMAGE, "--format", "{{.Size}}"]);
  console.log(`Image ${IMAGE}: ${Math.round(size / 1024 / 1024)} MB to download, ${onDisk} unpacked\n`);
  const pct = (v) => `${Math.round(v * 100)}%`;
  const rows = [
    ["Start-up, to the windows on the display", "startUpMs", (v) => `${(v / 1000).toFixed(2)} s`],
    ["Processes", "processes", String],
    ["Container memory", "containerMb", (v) => `${v} MB`],
    ["Of which the terminal (shell and WebKit)", "terminalMb", (v) => `${v} MB`],
    ["Of which the shell process (Rust core, GTK, WebKit's UI side)", (r) => r.pssMb.shell, (v) => `${v} MB`],
    ["Of which the display and VNC", (r) => r.pssMb.display, (v) => `${v} MB`],
    ["Processor, container, share of one core", "containerCpu", pct],
    ["Processor, the terminal", (r) => r.cpu.shell + r.cpu.webkit, pct],
    ["Processor, the display and VNC", (r) => r.cpu.display, pct],
  ];
  console.log(`| | ${SCENARIOS.map((s) => s.name).join(" | ")} |`);
  console.log(`|---|${SCENARIOS.map(() => "---:").join("|")}|`);
  for (const [label, key, format] of rows) {
    const pick = typeof key === "function" ? key : (r) => r[key];
    console.log(`| ${label} | ${SCENARIOS.map((s) => range(results[s.name].map(pick), format)).join(" | ")} |`);
  }
}

async function main() {
  const runsAt = process.argv.indexOf("--runs");
  const runs = runsAt > 0 ? Number(process.argv[runsAt + 1]) : 1;
  if (!Number.isInteger(runs) || runs < 1) throw new Error("--runs takes a whole number");
  docker(["image", "inspect", IMAGE]);
  const results = {};
  for (const scenario of SCENARIOS) {
    results[scenario.name] = [];
    for (let i = 0; i < runs; i++) {
      console.log(`footprint-docker: ${scenario.name}, run ${i + 1} of ${runs}`);
      results[scenario.name].push(await measure(scenario));
    }
  }
  report(results, runs);
}

main().catch((err) => {
  console.error(`footprint-docker: ${err.message}`);
  process.exit(1);
});
