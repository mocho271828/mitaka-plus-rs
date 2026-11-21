#!/usr/bin/env bash
# 実行ファイルを明示し、配布物でも同じWAV・親子通信・デコーダーを検証する。
set -euo pipefail
player="${1:?mitaka-media のパスを指定してください}"
shift
[[ -f "$player" ]] || { echo "missing player: $player" >&2; exit 1; }
player="$(cd "$(dirname "$player")" && pwd)/$(basename "$player")"
export MITAKA_TEST_MEDIA_HELPER="$player"
test_args=(cargo test --locked -p mitaka-desktop --bin mitaka-plus "$@" media_preview::worker::tests::real_player_controls_the_selected_handle -- --ignored --exact --nocapture)
case "$(uname -s)" in
    Linux)
        export XDG_RUNTIME_DIR PULSE_SERVER PULSE_SINK
        XDG_RUNTIME_DIR="$(mktemp -d)"
        PULSE_SERVER="unix:$XDG_RUNTIME_DIR/pulse/native"
        PULSE_SINK=mitaka_test
        pulseaudio -n --daemonize=no --exit-idle-time=-1 \
            -L module-native-protocol-unix -L "module-null-sink sink_name=$PULSE_SINK" >"$XDG_RUNTIME_DIR/pulse.log" 2>&1 &
        pulse_pid=$!
        cleanup() {
            kill "$pulse_pid" 2>/dev/null || true
            wait "$pulse_pid" 2>/dev/null || true
            rm -rf "$XDG_RUNTIME_DIR"
        }
        trap cleanup EXIT
        for _ in {1..100}; do
            if pactl get-sink-mute "$PULSE_SINK" >/dev/null 2>&1; then break; fi
            sleep 0.1
        done
        pactl get-sink-mute "$PULSE_SINK" || { cat "$XDG_RUNTIME_DIR/pulse.log" >&2; exit 1; }
        dbus-run-session -- xvfb-run -a -s '-screen 0 1280x800x24' "${test_args[@]}"
        ;;
    MINGW*|MSYS*|CYGWIN*)
        MITAKA_TEST_MEDIA_HELPER="$(cygpath -w "$player")"
        export WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS="--disable-audio-output"
        "${test_args[@]}"
        ;;
    Darwin) "${test_args[@]}" ;;
    *) echo "Unsupported media test platform" >&2; exit 1 ;;
esac
