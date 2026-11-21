#!/usr/bin/env bash
# Ubuntu 24.04 の Wayland と X11 のそれぞれで同じバイナリを起動する。
set -euo pipefail
app="$1"
catalog="$2"
export XDG_RUNTIME_DIR
XDG_RUNTIME_DIR="$(mktemp -d)"
weston --no-config --backend=headless --renderer=pixman --socket=mitaka-test --idle-time=0 >"$XDG_RUNTIME_DIR/weston.log" 2>&1 &
weston_pid=$!
cleanup() {
    kill "$weston_pid" 2>/dev/null || true
    wait "$weston_pid" 2>/dev/null || true
    rm -rf "$XDG_RUNTIME_DIR"
}
trap cleanup EXIT
for _ in {1..50}; do
    [[ ! -S "$XDG_RUNTIME_DIR/mitaka-test" ]] || break
    sleep 0.1
done
if [[ ! -S "$XDG_RUNTIME_DIR/mitaka-test" ]]; then cat "$XDG_RUNTIME_DIR/weston.log" >&2; exit 1; fi
env -u DISPLAY WAYLAND_DISPLAY=mitaka-test timeout 120 "$app" --smoke-test --data-root "$catalog"
env -u WAYLAND_DISPLAY xvfb-run -a -s '-screen 0 1280x800x24' timeout 120 "$app" --smoke-test --data-root "$catalog"
