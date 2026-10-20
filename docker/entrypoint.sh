#!/bin/sh
# Starts Fansipan in its container.
#
# With DISPLAY set (a Linux host sharing its X server), the window opens on
# the host. Without it, the terminal runs on a virtual display that noVNC
# serves to a browser on port 6080.
set -eu

# A container has no GPU: WebKit draws in software.
export WEBKIT_DISABLE_DMABUF_RENDERER=1
export WEBKIT_DISABLE_COMPOSITING_MODE=1
export LIBGL_ALWAYS_SOFTWARE=1
# No accessibility bus runs in the container; do not look for one.
export NO_AT_BRIDGE=1

if [ -n "${DISPLAY:-}" ]; then
  exec dbus-run-session -- fansipan
fi

export DISPLAY=:99
SCREEN="${FANSIPAN_SCREEN:-1600x1000}"

Xvfb "$DISPLAY" -screen 0 "${SCREEN}x24" -nolisten tcp &
tries=0
until xdpyinfo >/dev/null 2>&1; do
  tries=$((tries + 1))
  if [ "$tries" -gt 100 ]; then
    echo "fansipan: the virtual display did not start" >&2
    exit 1
  fi
  sleep 0.1
done

# A window manager, so the window can be moved and resized; it opens it
# maximized.
openbox --config-file /etc/fansipan/openbox.xml >/dev/null 2>&1 &
# VNC listens inside the container only; websockify is the one way in.
x11vnc -display "$DISPLAY" -forever -shared -nopw -localhost -quiet -rfbport 5900 >/dev/null 2>&1 &
websockify --web /usr/share/novnc 6080 localhost:5900 >/dev/null 2>&1 &

echo "fansipan: open http://localhost:6080/vnc.html?autoconnect=true&resize=scale"
exec dbus-run-session -- fansipan
