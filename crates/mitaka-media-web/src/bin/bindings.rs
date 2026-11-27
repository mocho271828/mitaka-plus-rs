//! 固定 wasm-bindgen の標準ローダーを生成する。Deno 出力の初期化は ESM・fetch・WebAssembly のみを使用する。
use std::{env, error::Error, path::PathBuf};
fn main() -> Result<(), Box<dyn Error>> {
    let mut args = env::args_os().skip(1);
    let input = PathBuf::from(
        args.next()
            .ok_or("usage: mitaka-media-bindings INPUT.wasm OUTPUT")?,
    );
    let out = PathBuf::from(args.next().ok_or("missing output directory")?);
    if args.next().is_some() {
        return Err("unexpected argument".into());
    }
    wasm_bindgen_cli_support::Bindgen::new()
        .input_path(input)
        .out_name("controls")
        .deno(true)?
        .typescript(false)
        .emit_start(true)
        .generate(out)?;
    Ok(())
}
