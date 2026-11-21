#!/usr/bin/env bash
# Rust 1.100.0 + wasm32-unknown-unknown の標準ライブラリを事前に用意する。
set -euo pipefail
cd "$(dirname "$0")/.."
case "${1:---check}" in --check|--write) mode="${1:---check}";; *) exit 2;; esac
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
cargo metadata --locked --offline --format-version 1 > "$tmp/metadata.json"
target="$(jq -r .target_directory "$tmp/metadata.json")"
flags="--remap-path-prefix=$PWD=/mitaka"
while IFS= read -r registry; do
    flags+=$'\x1f'"--remap-path-prefix=$registry=/cargo/source"
done < <(jq -r '.packages[] | select(.source != null) | .manifest_path | gsub("\\\\";"/") | split("/")[:-2] | join("/")' "$tmp/metadata.json" | sort -u)
CARGO_ENCODED_RUSTFLAGS="$flags" cargo build --frozen -p mitaka-media-web --lib --target wasm32-unknown-unknown --release
cargo run --frozen -p mitaka-media-web --features generate --bin mitaka-media-bindings -- "$target/wasm32-unknown-unknown/release/mitaka_media_web.wasm" "$tmp/out"
out=crates/mitaka-desktop/src/bin/mitaka-media/web
for file in controls.js controls_bg.wasm; do
    if [[ "$mode" == --write ]]; then cp "$tmp/out/$file" "$out/$file"; else cmp "$tmp/out/$file" "$out/$file"; fi
done
if [[ "$mode" == --write ]]; then
    (cd "$out" && if command -v sha256sum >/dev/null; then sha256sum controls.js controls_bg.wasm; else shasum -a256 controls.js controls_bg.wasm; fi) > crates/mitaka-media-web/SHA256SUMS
fi
