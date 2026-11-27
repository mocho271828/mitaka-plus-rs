use std::{
    fs,
    io::{Read, Write},
    path::Path,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};
use tempfile::tempdir;

const EXE: &str = env!("CARGO_BIN_EXE_mitaka-script");

#[test]
fn executes_trusted_local_source_and_preserves_standard_streams() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("星 図.lua");
    fs::write(dir.path().join("nested.lua"), "return 17").unwrap();
    fs::write(
        &path,
        r#"assert(_VERSION == "Lua 5.4" and debug and os and package)
assert(dofile("nested.lua") == 17)
assert(mtk.STEP_SEC == 0 and STEP_VALUE == 11 and UNIT_AU == mtk.UNIT_AU)
print(io.read("*l"))
io.stderr:write("error stream")
waitTime(0)
"#,
    )
    .unwrap();
    let mut child = Command::new(EXE)
        .arg(path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"passing input\n")
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap().trim_end(),
        "passing input"
    );
    assert_eq!(output.stderr, b"error stream");
}

#[cfg(target_os = "linux")]
#[test]
fn loads_a_native_module_against_the_helpers_lua_api() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("answer.c"),
        r#"#include "lauxlib.h"
int luaopen_answer(lua_State *L) {
    luaL_checkversion(L);
    lua_pushinteger(L, 42);
    return 1;
}
"#,
    )
    .unwrap();
    let compiler = Command::new("cc")
        .current_dir(dir.path())
        .args(["-shared", "-fPIC", "-I"])
        .arg(Path::new(env!("OUT_DIR")).join("lua-5.4.9/src"))
        .args(["answer.c", "-o", "answer.so"])
        .output()
        .unwrap();
    assert!(
        compiler.status.success(),
        "{}",
        String::from_utf8_lossy(&compiler.stderr)
    );
    let script = dir.path().join("probe.lua");
    fs::write(
        &script,
        "package.cpath = './?.so'\nlocal answer = require('answer')\nprint(answer)",
    )
    .unwrap();
    let output = Command::new(EXE).arg(script).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"42\n");
}

#[test]
fn reports_compile_runtime_file_and_resource_errors() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("bad.lua");
    let check = |source: &[u8], message: &str| {
        fs::write(&path, source).unwrap();
        let result = Command::new(EXE).arg(&path).output().unwrap();
        assert_eq!(result.status.code(), Some(1));
        let err = String::from_utf8_lossy(&result.stderr);
        assert!(err.contains(message), "{err}");
    };
    check(b"this is invalid!", "syntax error");
    check(b"error('test failure')", "test failure");
    check(b"while true do end", "budget exceeded");
    check(b"waitTime(-1)", "negative");
    check(b"return string.rep('a', 70 * 1024 * 1024)", "memory");
    check(&vec![b' '; 1024 * 1024 + 1], "1 MiB");
    check(b"\x1bLua", "binary");
    assert_eq!(Command::new(EXE).output().unwrap().status.code(), Some(2));
    assert_eq!(
        Command::new(EXE)
            .arg(dir.path())
            .output()
            .unwrap()
            .status
            .code(),
        Some(1)
    );
}

#[test]
fn running_script_can_be_cancelled_and_another_started() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("slow.lua");
    fs::write(
        &path,
        "local f=assert(io.open('ready','w')); f:close(); waitTime(100)",
    )
    .unwrap();
    let mut running = mitaka_script::spawn_helper(Path::new(EXE), &path, None).unwrap();
    let start = Instant::now();
    while !dir.path().join("ready").exists() && start.elapsed() < Duration::from_secs(5) {
        thread::sleep(Duration::from_millis(10));
    }
    let started = dir.path().join("ready").exists();
    running.kill().unwrap();
    assert!(!running.wait().unwrap().success());
    assert!(started);
    fs::write(&path, "assert(true)").unwrap();
    let mut next = mitaka_script::spawn_helper(Path::new(EXE), &path, None).unwrap();
    assert!(next.wait().unwrap().success());
}

#[test]
fn host_stream_preserves_snapshot_actions_and_final_status() {
    use interprocess::local_socket::{ListenerOptions, prelude::*};
    use serde_json::{Value, json};
    let dir = tempdir().unwrap();
    let file = dir.path().join("host.lua");
    fs::write(
        &file,
        r#"assert(Time.JD == 2451545)
assert(Camera.zoom == UNIT_AU)
assert(not pcall(function() Time.JD = "1" end))
Time.JD = 2451546
Stars:toggleLabels()
print("", Camera.target, "")
print(string.rep("星", 2731))
print(string.rep("x", 8190), "🪐")
print(string.rep("x", 8191) .. string.char(0xe3, 0x81))
print(string.char(255))
Time:stop()
Camera:setZoomLogTo(4*UNIT_AU)
Camera:setEulerTo(10,20,30,0.5,false)
local x,y,z,w=Camera:getOrient()
assert(x==0 and y==0 and z==0 and w==1)
assert(not pcall(function() Camera.setZoomTo(Stars,1,0) end))
assert(not pcall(function() Camera:setAzimuthTo(0,1,"bad") end))
"#,
    )
    .unwrap();
    #[cfg(unix)]
    let endpoint = dir.path().join("helper.sock").into_os_string();
    #[cfg(unix)]
    let name = endpoint
        .as_os_str()
        .to_fs_name::<interprocess::local_socket::GenericFilePath>()
        .unwrap();
    #[cfg(windows)]
    let endpoint = dir.path().file_name().unwrap().to_os_string();
    #[cfg(windows)]
    let name = endpoint
        .as_os_str()
        .to_ns_name::<interprocess::local_socket::GenericNamespaced>()
        .unwrap();
    let listener = ListenerOptions::new().name(name).create_sync().unwrap();
    let mut child = mitaka_script::spawn_helper(Path::new(EXE), &file, Some(&endpoint)).unwrap();
    let mut socket = listener.accept().unwrap();
    let mut actions = Vec::new();
    let mut logs = Vec::new();
    loop {
        let mut header = [0; 4];
        socket.read_exact(&mut header).unwrap();
        let size = u32::from_be_bytes(header) as usize;
        assert!((1..=65_536).contains(&size));
        let mut body = vec![0; size];
        socket.read_exact(&mut body).unwrap();
        let msg: Value = serde_json::from_slice(&body).unwrap();
        let reply = match msg["op"].as_str().unwrap() {
            "snapshot" => {
                json!({"ok":true, "jd":2451545.0, "distance":1.0, "starLabels":false, "target":"Planet.Earth", "cameraDuration":2.0, "cameraOrientation":[0.0,0.0,0.0,1.0]})
            }
            "apply" => {
                actions.push(msg);
                json!({"ok":true})
            }
            "log" => {
                logs.push(msg["text"].as_str().unwrap().to_owned());
                json!({"ok":true})
            }
            "done" => {
                assert_eq!(msg["ok"], true, "{msg}");
                break;
            }
            other => panic!("unexpected host request {other}"),
        };
        let payload = serde_json::to_vec(&reply).unwrap();
        socket
            .write_all(&(payload.len() as u32).to_be_bytes())
            .unwrap();
        socket.write_all(&payload).unwrap();
        socket.flush().unwrap();
    }
    assert!(child.wait().unwrap().success());
    assert_eq!(actions.len(), 5);
    assert_eq!(actions[3]["kind"], 103);
    assert_eq!(actions[3]["number"], 4.);
    assert_eq!(actions[3]["enabled"], true);
    assert_eq!(actions[3]["duration"], 2.);
    assert_eq!(actions[4]["kind"], 98);
    assert_eq!(actions[4]["orientation"], json!([10., 20., 30.]));
    assert_eq!(actions[4]["enabled"], false);
    assert_eq!(actions[4]["duration"], 0.5);
    assert_eq!(actions[0]["kind"], 0);
    assert_eq!(actions[0]["number"], 2451546.0);
    assert_eq!(actions[1]["kind"], 8);
    assert_eq!(actions[1]["enabled"], true);
    assert_eq!(actions[2]["kind"], 2);
    assert_eq!(
        logs,
        [
            "\tPlanet.Earth\t".to_owned(),
            "星".repeat(2730),
            format!("{}\t", "x".repeat(8190)),
            "[non-UTF-8 output]".to_owned(),
            "[non-UTF-8 output]".to_owned(),
        ]
    );
}
