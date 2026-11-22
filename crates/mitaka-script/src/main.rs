mod bindings;
mod host;

use mitaka_script as _;
use mlua::chunk::ChunkMode;
use mlua::{Error, HookTriggers, Lua, VmState};
use std::{
    cell::Cell,
    env,
    ffi::OsString,
    fs::{self, File},
    io::{self, Read},
    path::{Path, PathBuf},
    process::{self, ExitCode},
    thread,
    time::{Duration, Instant},
};

const SOURCE_LIMIT: u64 = 1024 * 1024;
const LUA_TIME: Duration = Duration::from_secs(110);
const HARD_LIMIT: Duration = Duration::from_secs(120);

fn run_script(
    path: &Path,
    host: Option<&host::SharedHost>,
) -> Result<(), Box<dyn std::error::Error>> {
    let path = fs::canonicalize(path)?;
    if !fs::metadata(&path)?.is_file() {
        return Err("script must be a regular file".into());
    }
    let file = File::open(&path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() > SOURCE_LIMIT {
        return Err("script must be a regular file of at most 1 MiB".into());
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(SOURCE_LIMIT + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > SOURCE_LIMIT {
        return Err("script exceeds 1 MiB".into());
    }
    if let Some(parent) = path.parent() {
        env::set_current_dir(parent)?;
    }
    // 選択されたローカルコードを独立したプロセスで実行する。旧版と同じく
    // ユーザーが許可した os・io・debug・C モジュールを含む標準機能を公開する。
    let lua = unsafe { Lua::unsafe_new() };
    lua.set_memory_limit(64 * 1024 * 1024)?;
    let started = Instant::now();
    let hooks = Cell::new(0_u32);
    lua.set_global_hook(
        HookTriggers::new().every_nth_instruction(1000),
        move |_, _| {
            let count = hooks.get() + 1;
            hooks.set(count);
            if count > 10_000 || started.elapsed() > LUA_TIME {
                return Err(Error::runtime("script execution budget exceeded"));
            }
            Ok(VmState::Continue)
        },
    )?;
    lua.globals().set(
        "waitTime",
        lua.create_function(move |_, value: mlua::Value| {
            let seconds = match value {
                mlua::Value::Integer(n) => n as f64,
                mlua::Value::Number(n) => n,
                _ => return Err(Error::runtime("waitTime expects a number")),
            };
            let wait = Duration::try_from_secs_f64(seconds).map_err(Error::external)?;
            if wait > LUA_TIME.saturating_sub(started.elapsed()) {
                return Err(Error::runtime("script execution budget exceeded"));
            }
            thread::sleep(wait);
            Ok(())
        })?,
    )?;
    host::install(&lua, host)?;
    lua.load(bytes)
        .set_name(format!("@{}", path.display()))
        .set_mode(ChunkMode::Text)
        .exec()?;
    Ok(())
}

struct Options {
    host: Option<OsString>,
    script: PathBuf,
}

fn options() -> Result<Option<Options>, &'static str> {
    let mut args = env::args_os().skip(1);
    let mut host = None;
    let mut literal = false;
    while let Some(arg) = args.next() {
        if !literal && arg == "--" {
            literal = true;
            continue;
        }
        if !literal && (arg == "--help" || arg == "-h") {
            return Ok(None);
        }
        if !literal && arg == "--host" {
            if host.is_some() {
                return Err("duplicate --host");
            }
            host = Some(args.next().ok_or("missing host endpoint")?);
            continue;
        }
        if !literal && arg.to_string_lossy().starts_with('-') {
            return Err("unknown option");
        }
        if args.next().is_some() {
            return Err("expected a single script");
        }
        return Ok(Some(Options {
            host,
            script: arg.into(),
        }));
    }
    Err("missing script")
}

fn main() -> ExitCode {
    let usage = "mitaka-script [--host LOCAL_ENDPOINT] [--] FILE.lua";
    let args = match options() {
        Ok(Some(args)) => args,
        Ok(None) => {
            println!("{usage}");
            return ExitCode::SUCCESS;
        }
        Err(e) => {
            eprintln!("{e}\nUsage: {usage}");
            return ExitCode::from(2);
        }
    };
    // C 関数内のブロックや debug.sethook にも旧ホストと同じ終了期限を適用する。
    thread::spawn(|| {
        thread::sleep(HARD_LIMIT);
        eprintln!("script wall-time limit exceeded");
        process::exit(1);
    });
    let connection = match args.host.as_deref().map(host::connect).transpose() {
        Ok(host) => host,
        Err(e) => {
            eprintln!("cannot connect to script host: {e}");
            return ExitCode::FAILURE;
        }
    };
    let result = run_script(&args.script, connection.as_ref());
    let error = result.err().map(|e| e.to_string());
    let report: io::Result<()> = match &connection {
        Some(host) => host::complete(
            host,
            error.is_none(),
            error.as_deref().unwrap_or("スクリプトを実行しました。"),
        ),
        None => Ok(()),
    };
    if let Some(message) = error {
        eprintln!("{message}");
        return ExitCode::FAILURE;
    }
    if let Err(e) = report {
        eprintln!("cannot notify script host: {e}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
