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
    CARGO_HOME="$out/empty-cargo-home" cargo metadata --frozen --format-version 1 > /dev/null
)
tar -czf "dist/mitaka-plus-$ref-source.tar.gz" -C "$out" "mitaka-plus-$ref"
