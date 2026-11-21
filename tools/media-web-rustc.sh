#!/usr/bin/env bash
# Cargo のホスト固有 fingerprint を Wasm の外部シンボル名へ流さない。
set -euo pipefail
rustc="$1"
shift
args=("$@")
wasm=false
metadata_index=
for ((i=0; i<${#args[@]}; i++)); do
    if [[ "${args[i]}" == --target && "${args[i+1]:-}" == wasm32-unknown-unknown ]]; then wasm=true; fi
    if [[ "${args[i]}" == -C && "${args[i+1]:-}" == metadata=* ]]; then metadata_index=$((i+1)); fi
done
if [[ "$wasm" == true && -n "$metadata_index" ]]; then
    # 対象は Cargo.lock で固定した単一の wasm32 依存グラフ。ホスト用 proc-macro 等は通常どおり。
    args[metadata_index]="metadata=mitaka-media-${CARGO_PKG_NAME:?}-${CARGO_PKG_VERSION:?}"
fi
exec "$rustc" "${args[@]}"
