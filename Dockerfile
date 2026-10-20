# syntax=docker/dockerfile:1.7
# Fansipan in a container, so it can be tried without installing Rust, Node,
# or Tauri's system libraries:
#
#   docker compose up --build
#   # then open http://localhost:6080/vnc.html?autoconnect=true&resize=scale
#
# The terminal runs on a virtual display inside the container and noVNC
# shows it in the browser, which works with Docker Desktop on Windows and
# macOS as well as on Linux. On a Linux desktop it can open its own window
# on the host's X server instead (see README.md, "Run with Docker").
#
# The image runs the sandbox only: simulated markets, no broker.

ARG RUST_VERSION=1
ARG NODE_VERSION=22
ARG PNPM_VERSION=10.15.0

# ── Front end ────────────────────────────────────────────────────────────────
FROM node:${NODE_VERSION}-bookworm-slim AS frontend
ARG PNPM_VERSION
ENV COREPACK_ENABLE_DOWNLOAD_PROMPT=0
RUN corepack enable && corepack prepare "pnpm@${PNPM_VERSION}" --activate
WORKDIR /src/crates/ui-shell/frontend
COPY crates/ui-shell/frontend/package.json crates/ui-shell/frontend/pnpm-lock.yaml ./
RUN pnpm install --frozen-lockfile
COPY crates/ui-shell/frontend/ ./
RUN pnpm build

# ── Shell ────────────────────────────────────────────────────────────────────
# Tauri's Linux build needs WebKitGTK 4.1 and GTK 3, as listed in Tauri's
# prerequisites for Debian.
FROM rust:${RUST_VERSION}-bookworm AS builder
RUN apt-get update && apt-get install -y --no-install-recommends \
        libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev \
        librsvg2-dev libssl-dev libxdo-dev pkg-config file \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY --from=frontend /src/crates/ui-shell/frontend/dist ./crates/ui-shell/frontend/dist
# custom-protocol embeds the built front end in the binary, as `tauri build`
# does; without it the shell would look for a dev server.
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release --locked -p fansipan-shell --features tauri/custom-protocol \
    && cp target/release/fansipan-shell /usr/local/bin/fansipan

# ── Runtime ──────────────────────────────────────────────────────────────────
FROM debian:bookworm-slim AS runtime
RUN apt-get update && apt-get install -y --no-install-recommends \
        libwebkit2gtk-4.1-0 libgtk-3-0 libayatana-appindicator3-1 librsvg2-2 libxdo3 \
        ca-certificates fonts-dejavu-core dbus tzdata \
        xvfb x11-utils openbox x11vnc novnc websockify \
    && rm -rf /var/lib/apt/lists/*
# Xvfb runs as the app's user, who cannot create the X socket directory.
RUN useradd --create-home --uid 1000 fansipan \
    && mkdir -p /home/fansipan/.config /tmp/.X11-unix \
    && chmod 1777 /tmp/.X11-unix \
    && chown -R fansipan:fansipan /home/fansipan
COPY --from=builder /usr/local/bin/fansipan /usr/local/bin/fansipan
COPY docker/openbox.xml /etc/fansipan/openbox.xml
COPY docker/entrypoint.sh /usr/local/bin/fansipan-entrypoint
# A checkout on Windows may have given the script CRLF line endings.
RUN sed -i 's/\r$//' /usr/local/bin/fansipan-entrypoint \
    && chmod +x /usr/local/bin/fansipan-entrypoint
USER fansipan
WORKDIR /home/fansipan
EXPOSE 6080
ENTRYPOINT ["/usr/local/bin/fansipan-entrypoint"]
