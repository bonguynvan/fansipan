# Fansipan

An open-source desktop trading terminal for securities, on any market and
with any broker. The core never knows which broker it is talking to, or
which market's rules it trades under: everything broker-specific lives in
an **adapter**, and every real-world adapter lives in its own repository,
never in this one.

This repository holds the engine, the domain model, the desktop shell
(Rust, Tauri, and Svelte), the trait contract adapters implement, and one
simulated adapter, the sandbox. No real broker is connected from here.

## What it does

- **Markets as data.** Lot sizes, tick ladders, daily price limits,
  auction sessions, the order types each session takes, settlement, and
  short-selling rules come from the adapter. The ticket, the risk checks,
  and the screens follow them.
- **Two simulated markets.** The sandbox runs a demanding market, with daily
  ceilings and floors, opening and closing auctions, lots of 100, odd lots
  and settlement two days after purchase, on three exchanges. A simpler,
  US-style market runs beside it, to show the same terminal under
  different rules.
- **Orders the engine owns.** Adapters only report what the venue says;
  the engine decides each order's state, handles the venue's races, and
  checks risk before an order ever reaches an adapter.
- **A workspace you arrange.** Panels you drag to split, stack, and
  resize, on pages and desktops, all restored on restart. Search (Ctrl+K)
  finds any command or symbol.
- **In more than one language.** English and Vietnamese; a translation is
  one JSON file.

## What it costs

Measured on a release build under Windows 11 (Intel Core i7-14700, 32 GB)
by `node scripts/footprint.mjs`:

- **11 MB** to install: one binary with the interface inside it. WebView2,
  which draws the screens, comes with Windows.
- **Under a second** from launch to the first prices on the board: about
  0.6 s.
- **Idle:** about 290 MB of private memory and 6 to 7% of one core. The
  Rust core is 9 MB of that; the rest is WebView2.
- **A busy feed (1,000 to 1,600 quotes a second):** about 450 to 520 MB of
  private memory, under 450 MB once its garbage is collected, and under
  half of one core.
- **In the Docker demo** (`node scripts/footprint-docker.mjs`): a 322 MB
  image, about half a second to the window, and about 290 MB for the
  container idle, 360 MB under the busy feed. With no GPU there, WebKit
  draws in software: about a quarter of a core idle, just over one busy.

## Run with Docker

Nothing else to install:

```bash
docker compose up --build
```

Then open <http://localhost:6080/vnc.html?autoconnect=true&resize=scale>.

- The terminal runs in the container on a virtual display, and noVNC
  shows it in your browser. This works with Docker Desktop on Windows and
  macOS, and with Docker on Linux. The first build takes several minutes.
- At first launch the terminal asks which simulated market to run and
  remembers it; **File › Market** changes it later.
  `FANSIPAN_SANDBOX_MARKET=us` (or `vn`) skips the question.
- `FANSIPAN_SANDBOX_CLOCK=real` keeps real trading hours instead of the
  demo day, which compresses a whole session into 15 minutes.
- `FANSIPAN_SANDBOX_BEAT_MS=10` moves the market every 10 ms instead of
  every 500, to see the terminal under load.
- `TZ=<zone>` sets the clock the chart shows, which is UTC by default.
- The page is served to this machine only, because the VNC session behind
  it has no password. Do not publish port 6080 further.

On a Linux desktop, the image can open the terminal's own window on your X
server instead:

```bash
docker run --rm --shm-size 512m -e DISPLAY=$DISPLAY -v /tmp/.X11-unix:/tmp/.X11-unix fansipan:local
```

## Run natively

You need rustup (`rust-toolchain.toml` pins the Rust version), Node with
pnpm, and [Tauri's prerequisites](https://tauri.app/start/prerequisites/)
for your OS; on Linux, the packages [CI](.github/workflows/ci.yml)
installs.

```bash
cd crates/ui-shell/frontend && pnpm install && cd ../../..
cd crates/ui-shell && ./frontend/node_modules/.bin/tauri dev
```

The first run compiles the shell and takes several minutes.

## Tests

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
python scripts/check-workspace.py   # every crate is a workspace member
cargo deny check                    # licences, advisories, sources
cd crates/ui-shell/frontend && pnpm run check && pnpm test
```

Everything runs offline and deterministically; no test needs a venue. CI
runs these on Linux and Windows for every push.

## Contributing

Sign off each commit (`git commit -s`): it certifies the
[Developer Certificate of Origin](https://developercertificate.org/) for
your change. Never add a broker's wire protocol, captures, or credentials
to this repository; they belong in that broker's adapter.

## Licence

LGPL-3.0-or-later: see [COPYING.LESSER](COPYING.LESSER) and
[COPYING](COPYING). An adapter in its own repository chooses its own
licence.

The tables are [bo-grid](https://github.com/bonguynvan/bo-grid) and the
chart is [TradeCanvas](https://github.com/bonguynvan/tradecanvas).
