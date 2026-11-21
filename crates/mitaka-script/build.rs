use flate2::read::GzDecoder;
use sha2::{Digest, Sha256};
use std::{env, fs, io, path::PathBuf};

const ARCHIVE: &str = "vendor/lua-5.4.9.tar.gz";
const EXPECTED: &str = "2335b6c582a52654f94612bf10d2f4672805d05329aa6568b1d8cd9e5c6fb8e6";

fn main() -> io::Result<()> {
    println!("cargo:rerun-if-changed={ARCHIVE}");
    let archive = fs::read(ARCHIVE)?;
    if format!("{:x}", Sha256::digest(&archive)) != EXPECTED {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Lua 5.4.9 archive SHA-256 mismatch",
        ));
    }
    let output = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo sets OUT_DIR"));
    tar::Archive::new(GzDecoder::new(archive.as_slice())).unpack(&output)?;
    let source = output.join("lua-5.4.9/src");
    let mut build = cc::Build::new();
    build.include(&source).warnings(false).opt_level(2);
    match env::var("CARGO_CFG_TARGET_OS").as_deref() {
        Ok("macos") => {
            build.define("LUA_USE_MACOSX", None);
        }
        Ok("linux") => {
            build.define("LUA_USE_LINUX", None);
            println!("cargo:rustc-link-lib=dl");
            println!("cargo:rustc-link-lib=m");
            // Lua の Linux 用インタプリタと同様に、C 拡張が使う公開 API を解決可能にする。
            for symbols in ["lua_*", "luaL_*", "luaopen_*"] {
                println!(
                    "cargo:rustc-link-arg-bin=mitaka-script=-Wl,--export-dynamic-symbol={symbols}"
                );
            }
        }
        Ok("windows") => {
            // luaconf.h はコンパイラが指定する _WIN32 で Windows 機能を選択する。
        }
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "unsupported Lua target OS",
            ));
        }
    }
    let mut files = Vec::new();
    for entry in fs::read_dir(source)? {
        let path = entry?.path();
        if path.extension().is_some_and(|ext| ext == "c")
            && !matches!(
                path.file_name().and_then(|v| v.to_str()),
                Some("lua.c" | "luac.c")
            )
        {
            files.push(path);
        }
    }
    files.sort();
    build.files(files).compile("mitaka_lua54");
    Ok(())
}
