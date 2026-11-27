#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
out="$(mktemp -d)"
trap 'rm -rf "$out"' EXIT
ref="$(git rev-parse --short=12 HEAD)"
tree="$out/mitaka-plus-$ref"
mkdir -p "$tree" dist
git archive HEAD | tar -xf - -C "$tree"
(
    cd "$tree"
    mkdir -p .cargo
    cargo vendor --locked --versioned-dirs crates-vendor > .cargo/config.toml
)
archive="mitaka-plus-$ref-source.tar.gz"
tar -czf "$out/$archive" -C "$out" "mitaka-plus-$ref"

# 実際に配るアーカイブから、作成時の依存キャッシュやビルド済みバイナリを参照せずに検証する。
mkdir -p "$out/unpacked" "$out/empty-cargo-home"
tar -xzf "$out/$archive" -C "$out/unpacked"
(
    cd "$out/unpacked/mitaka-plus-$ref"
    CARGO_HOME="$out/empty-cargo-home" CARGO_TARGET_DIR="$out/target" cargo build --workspace --frozen
)
mv -f "$out/$archive" "dist/$archive"
