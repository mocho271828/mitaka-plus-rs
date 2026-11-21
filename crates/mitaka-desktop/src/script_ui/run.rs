use command_group::{CommandGroup, GroupChild};
use interprocess::local_socket::{
    prelude::*, Listener, ListenerNonblockingMode, ListenerOptions, Stream,
};
use serde_json::Value;
use std::{
    env,
    ffi::OsString,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    thread,
    time::{Duration, Instant},
};
use tempfile::TempDir;

pub enum Event {
    Request(Value, mpsc::Sender<Value>),
    End(Result<String, String>),
}

const WALL_LIMIT: Duration = Duration::from_secs(120);
const FRAME_LIMIT: usize = 65_536;
fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
pub fn scratch() -> io::Result<TempDir> {
    let mut b = tempfile::Builder::new();
    b.prefix("mitaka-lua-");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        b.permissions(std::fs::Permissions::from_mode(0o700));
    }
    b.tempdir()
}

pub struct Run {
    /// 中断・新規実行をまたいで以前の操作を適用しないよう、受信箱を実行ごとに所有する。
    pub events: mpsc::Receiver<Event>,
    pub start: Instant,
    child: GroupChild,
    stopped: Arc<AtomicBool>,
    _scratch: TempDir,
}

fn endpoint(dir: &Path) -> io::Result<(OsString, Listener)> {
    #[cfg(unix)]
    let addr = dir.join("ipc").into_os_string();
    #[cfg(unix)]
    let name = addr
        .as_os_str()
        .to_fs_name::<interprocess::local_socket::GenericFilePath>()?;
    #[cfg(windows)]
    let addr = dir
        .file_name()
        .ok_or_else(|| invalid("invalid temporary directory"))?
        .to_owned();
    #[cfg(windows)]
    let name = addr
        .as_os_str()
        .to_ns_name::<interprocess::local_socket::GenericNamespaced>()?;
    let listener = ListenerOptions::new()
        .name(name)
        .nonblocking(ListenerNonblockingMode::Accept)
        .create_sync()?;
    Ok((addr, listener))
}

fn peer_id(conn: &Stream) -> io::Result<u32> {
    #[cfg(target_os = "macos")]
    {
        let Stream::UdSocket(socket) = conn;
        let id =
            nix::sys::socket::getsockopt(socket.inner(), nix::sys::socket::sockopt::LocalPeerPid)?;
        u32::try_from(id).map_err(|_| invalid("invalid peer PID"))
    }
    #[cfg(target_os = "linux")]
    {
        conn.peer_creds()?
            .pid()
            .and_then(|id| u32::try_from(id).ok())
            .ok_or_else(|| invalid("missing peer PID"))
    }
    #[cfg(target_os = "windows")]
    {
        conn.peer_creds()?
            .pid()
            .ok_or_else(|| invalid("missing peer PID"))
    }
}

fn listen(
    listener: Listener,
    id: u32,
    stopped: &AtomicBool,
    tx: &mpsc::Sender<Event>,
    context: &eframe::egui::Context,
) -> io::Result<Result<String, String>> {
    let connect_start = Instant::now();
    let mut conn = loop {
        if connect_start.elapsed() > Duration::from_secs(10) {
            return Err(io::ErrorKind::TimedOut.into());
        }
        if stopped.load(Ordering::Acquire) {
            return Err(io::ErrorKind::Interrupted.into());
        }
        match listener.accept() {
            Ok(stream) if peer_id(&stream)? == id => break stream,
            Ok(_) => continue,
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(10))
            }
            Err(e) => return Err(e),
        }
    };
    // Darwin の accept は待受側の O_NONBLOCK を継承するため、接続後は明示的に戻す。
    conn.set_nonblocking(false)?;
    #[cfg(unix)]
    conn.set_recv_timeout(Some(WALL_LIMIT))?;
    loop {
        let mut header = [0; 4];
        conn.read_exact(&mut header)?;
        let size = u32::from_be_bytes(header) as usize;
        if !(1..=FRAME_LIMIT).contains(&size) {
            return Err(invalid("invalid Lua frame length"));
        }
        let mut bytes = vec![0; size];
        conn.read_exact(&mut bytes)?;
        let msg: Value = serde_json::from_slice(&bytes)?;
        if msg["op"] == "done" {
            let ok = msg["ok"]
                .as_bool()
                .ok_or_else(|| invalid("missing completion status"))?;
            let text = msg["message"]
                .as_str()
                .ok_or_else(|| invalid("missing completion message"))?
                .to_owned();
            return Ok(if ok { Ok(text) } else { Err(text) });
        }
        let (reply, response) = mpsc::channel();
        tx.send(Event::Request(msg, reply))
            .map_err(|_| io::ErrorKind::BrokenPipe)?;
        context.request_repaint();
        let reply = response
            .recv_timeout(WALL_LIMIT)
            .map_err(|_| io::ErrorKind::TimedOut)?;
        let payload = serde_json::to_vec(&reply)?;
        if payload.len() > FRAME_LIMIT {
            return Err(invalid("host response exceeds 64 KiB"));
        }
        conn.write_all(&(payload.len() as u32).to_be_bytes())?;
        conn.write_all(&payload)?;
        conn.flush()?;
    }
}

impl Run {
    pub fn start(
        script: PathBuf,
        working_dir: PathBuf,
        temp: TempDir,
        ctx: &eframe::egui::Context,
    ) -> io::Result<Self> {
        let mut helper = env::current_exe()?;
        helper.set_file_name(format!("mitaka-script{}", env::consts::EXE_SUFFIX));
        Self::start_with(&helper, script, working_dir, temp, ctx)
    }

    fn start_with(
        helper: &Path,
        script: PathBuf,
        working_dir: PathBuf,
        temp: TempDir,
        ctx: &eframe::egui::Context,
    ) -> io::Result<Self> {
        let (addr, listener) = endpoint(temp.path())?;
        let child = Command::new(helper)
            .arg("--host")
            .arg(addr)
            .arg("--working-dir")
            .arg(working_dir)
            .arg("--")
            .arg(script)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .group_spawn()?;
        let (tx, events) = mpsc::channel();
        let stopped = Arc::new(AtomicBool::new(false));
        let flag = stopped.clone();
        let id = child.id();
        let ctx = ctx.clone();
        thread::spawn(move || {
            let status = listen(listener, id, &flag, &tx, &ctx)
                .unwrap_or_else(|e| Err(format!("Lua 通信を終了しました: {e}")));
            let _ = tx.send(Event::End(status));
            ctx.request_repaint();
        });
        Ok(Self {
            child,
            events,
            start: Instant::now(),
            stopped,
            _scratch: temp,
        })
    }

    pub fn deadline_reached(&self) -> bool {
        self.start.elapsed() >= WALL_LIMIT
    }
}
impl Drop for Run {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Release);
        // 正常終了直後にも子で起動されたプロセスが残る場合がある。
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    #[ignore = "requires the built mitaka-script helper in MITAKA_TEST_SCRIPT_HELPER"]
    fn drives_real_helper_and_reaps_it() {
        let helper = PathBuf::from(
            env::var_os("MITAKA_TEST_SCRIPT_HELPER").expect("provide the built helper path"),
        );
        let work = scratch().unwrap();
        let script = work.path().join("host.lua");
        std::fs::write(
            &script,
            r#"local f=assert(io.open("relative.txt"));assert(f:read("*a")=="fixture"); f:close()
assert(Time.JD == 2451545 and Constels.lines)
Constels:toggleLines()
Time.JD = 2451546
assert(not Constels.lines and Time.JD == 2451546)
assert(not pcall(function() Time:rewind() end))
print("completed")"#,
        )
        .unwrap();
        std::fs::write(work.path().join("relative.txt"), "fixture").unwrap();
        let job = Run::start_with(
            &helper,
            script,
            work.path().to_owned(),
            scratch().unwrap(),
            &Default::default(),
        )
        .unwrap();
        let mut jd = 2451545.;
        let mut lines = true;
        let mut log = String::new();
        loop {
            let event = job.events.recv_timeout(Duration::from_secs(5)).unwrap();
            match event {
                Event::Request(msg, answer) => {
                    let reply = match msg["op"].as_str().unwrap() {
                        "snapshot" => json!({"ok":true,"jd":jd,"lines":lines}),
                        "apply" => match msg["kind"].as_u64().unwrap() {
                            5 => {
                                lines = msg["enabled"].as_bool().unwrap();
                                json!({"ok":true})
                            }
                            0 => {
                                jd = msg["number"].as_f64().unwrap();
                                json!({"ok":true})
                            }
                            _ => json!({"ok":false,"error":"unsupported"}),
                        },
                        "log" => {
                            log = msg["text"].as_str().unwrap().into();
                            json!({"ok":true})
                        }
                        op => panic!("unexpected operation {op}"),
                    };
                    answer.send(reply).unwrap();
                }
                Event::End(status) => {
                    status.unwrap();
                    break;
                }
            }
        }
        assert_eq!(jd, 2451546.);
        assert!(!lines);
        assert_eq!(log, "completed");
    }
}
