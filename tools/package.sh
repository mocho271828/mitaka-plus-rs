#!/usr/bin/env bash
# GitHub の各ネイティブランナーおよび開発用 Mac で実行する。
set -euo pipefail
cd "$(dirname "$0")/.."
mkdir -p dist
out="$(mktemp -d "$PWD/dist/.package.XXXXXX")"
trap 'rm -rf "$out"' EXIT
cargo metadata --locked --format-version 1 > "$out/metadata.json"
target_dir="$(jq -r '.target_directory | gsub("\\\\"; "/")' "$out/metadata.json")"
version="$(jq -r '.packages[] | select(.name == "mitaka-desktop") | .version' "$out/metadata.json")"
mkdir -p "$out/licenses"
cp LICENSE licenses/Mitaka.txt crates/mitaka-script/vendor/LUA_LICENSE.txt "$out/licenses/"
# バイナリに取り込む依存の許諾文も配布物から読めるようにする。
jq -r '.packages[] | select(.source != null) | [.name, .version, (.manifest_path | gsub("\\\\"; "/"))] | @tsv' "$out/metadata.json" |
while IFS=$'\t' read -r name ver manifest; do
    package_dir="$(dirname "$manifest")"
    dest="$out/licenses/$name-$ver"
    while IFS= read -r -d '' notice; do
        relative="${notice#"$package_dir"/}"
        mkdir -p "$dest/$(dirname "$relative")"
        cp "$notice" "$dest/$relative"
    done < <(find "$package_dir" -maxdepth 2 -type f \( -iname 'license*' -o -iname 'licence*' -o -iname 'copying*' -o -iname 'OFL*.txt' \) -print0)
done
case "$(uname -s)" in
    Darwin)
        export MACOSX_DEPLOYMENT_TARGET=13.0
        app="$out/Mitaka Plus.app"
        mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
        for arch in aarch64 x86_64; do
            cargo build --release --locked --workspace --bins --target "$arch-apple-darwin"
        done
        for binary in mitaka-plus mitaka-script; do
            lipo -create "$target_dir/aarch64-apple-darwin/release/$binary" "$target_dir/x86_64-apple-darwin/release/$binary" -output "$app/Contents/MacOS/$binary"
        done
        sed "s/@VERSION@/$version/g" packaging/Info.plist > "$app/Contents/Info.plist"
        cp -R "$out/licenses" "$app/Contents/Resources/"
        codesign --force --sign - "$app/Contents/MacOS/mitaka-script"
        codesign --force --sign - "$app"
        codesign --verify --strict --deep "$app"
        package="mitaka-plus-$version-macos-universal.zip"
        ditto -c -k --keepParent "$app" "$out/$package"
        ;;
    Linux)
        cargo build --release --locked --workspace --bins
        root="$out/deb"
        mkdir -p "$root/DEBIAN" "$root/usr/bin" "$root/usr/share/doc/mitaka-plus" "$root/usr/share/applications"
        install -m 755 "$target_dir/release/mitaka-plus" "$target_dir/release/mitaka-script" "$root/usr/bin/"
        cp -R "$out/licenses" "$root/usr/share/doc/mitaka-plus/"
        cp packaging/mitaka-plus.desktop "$root/usr/share/applications/"
        sed "s/@VERSION@/$version/g" packaging/debian-control > "$root/DEBIAN/control"
        package="mitaka-plus_${version}_amd64.deb"
        dpkg-deb --root-owner-group --build "$root" "$out/$package"
        ;;
    MINGW*|MSYS*|CYGWIN*)
        cargo build --release --locked --workspace --bins
        app="$out/mitaka-plus"
        mkdir -p "$app"
        cp "$target_dir/release/mitaka-plus.exe" "$target_dir/release/mitaka-script.exe" "$app/"
        cp -R "$out/licenses" "$app/"
        package="mitaka-plus-$version-windows-x64.zip"
        (cd "$out" && 7z a -tzip "$package" mitaka-plus)
        ;;
    *) echo "unsupported packaging platform" >&2; exit 1 ;;
esac
# 同じ版を作り直した場合も、今回生成したファイルだけを配布する。
mv -f "$out/$package" "dist/$package"
