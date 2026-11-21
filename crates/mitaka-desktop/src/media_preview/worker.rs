use serde_json::Value;
use std::{
    fs::File,
    io::{self, BufRead, BufReader, Read, Write},
    net::{Ipv4Addr, SocketAddrV4, TcpStream},
    path::Path,
    process::{Child, Command, Stdio},
    sync::mpsc::{self, Receiver},
    time::Duration,
};
pub struct Worker {
    child: Child,
    endpoint: Option<(SocketAddrV4, String)>,
    hello: Receiver<io::Result<(SocketAddrV4, String)>>,
    pub state: Value,
}
fn failure(s: &str) -> io::Error {
    io::Error::other(s)
}
impl Worker {
    pub fn start(file: File, path: &Path) -> io::Result<Self> {
        let helper = std::env::current_exe()?
            .with_file_name(format!("mitaka-media{}", std::env::consts::EXE_SUFFIX));
        Self::launch(&helper, file, path)
    }
    fn launch(helper: &Path, file: File, path: &Path) -> io::Result<Self> {
        let mut child = Command::new(helper)
            .arg("--selected-file")
            .arg(
                path.file_name()
                    .ok_or_else(|| failure("invalid filename"))?,
            )
            .stdin(Stdio::from(file))
            .stdout(Stdio::piped())
            .spawn()?;
        let stdout = child.stdout.take().expect("piped");
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let parse = || -> io::Result<(SocketAddrV4, String)> {
                let mut line = String::new();
                BufReader::new(stdout).take(120).read_line(&mut line)?;
                let (addr, key) = line
                    .trim()
                    .split_once('/')
                    .ok_or_else(|| failure("invalid player endpoint"))?;
                let addr: SocketAddrV4 = addr.parse().map_err(io::Error::other)?;
                if *addr.ip() != Ipv4Addr::LOCALHOST
                    || addr.port() == 0
                    || key.len() != 32
                    || !key.bytes().all(|c| c.is_ascii_hexdigit())
                {
                    return Err(failure("invalid player endpoint"));
                }
                Ok((addr, format!("/{key}")))
            };
            let _ = tx.send(parse());
        });
        Ok(Self {
            child,
            endpoint: None,
            hello: rx,
            state: Value::Null,
        })
    }
    pub fn refresh(&mut self) -> io::Result<bool> {
        if let Some(status) = self.child.try_wait()? {
            return if status.success() {
                Ok(false)
            } else {
                Err(failure("メディアプレーヤーが異常終了しました"))
            };
        }
        if self.endpoint.is_none() {
            match self.hello.try_recv() {
                Ok(v) => self.endpoint = Some(v?),
                Err(mpsc::TryRecvError::Empty) => return Ok(true),
                Err(_) => return Err(failure("プレーヤー初期化を確認できません")),
            }
        }
        self.state = self.request(serde_json::json!({"op":"status"}))?;
        Ok(true)
    }
    pub fn request(&self, msg: Value) -> io::Result<Value> {
        let (addr, path) = self
            .endpoint
            .as_ref()
            .ok_or_else(|| failure("メディアを準備中です"))?;
        let wait = Duration::from_millis(500);
        let mut socket = TcpStream::connect_timeout(&(*addr).into(), wait)?;
        socket.set_read_timeout(Some(wait))?;
        socket.set_write_timeout(Some(wait))?;
        let body = msg.to_string();
        write!(socket,"POST {path} HTTP/1.0\r\nHost: {addr}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len())?;
        let mut bytes = vec![];
        socket.take(4097).read_to_end(&mut bytes)?;
        if bytes.len() > 4096 {
            return Err(failure("oversized player reply"));
        }
        let offset = bytes
            .windows(4)
            .position(|x| x == b"\r\n\r\n")
            .ok_or_else(|| failure("invalid player reply"))?;
        let reply: Value =
            serde_json::from_slice(&bytes[offset + 4..]).map_err(io::Error::other)?;
        if reply["ok"] != true {
            return Err(failure(
                reply["error"].as_str().unwrap_or("player command failed"),
            ));
        }
        Ok(reply)
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    #[ignore = "requires the built media player and a graphical desktop"]
    fn real_player_controls_the_selected_handle() {
        let exe = std::env::var_os("MITAKA_TEST_MEDIA_HELPER").expect("MITAKA_TEST_MEDIA_HELPER");
        let tmp = tempfile::tempdir().unwrap();
        let wav = tmp.path().join("selected.wav");
        let mut writer = hound::WavWriter::create(
            &wav,
            hound::WavSpec {
                channels: 1,
                sample_rate: 16000,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            },
        )
        .unwrap();
        for sample in 0..96000 {
            // 無音検出で物理出力が停止する実装でも、PCMを最後まで処理する入力。
            let pcm = if sample % 64 < 32 { 32_i16 } else { -32 };
            writer.write_sample(pcm).unwrap();
        }
        writer.finalize().unwrap();
        let mut cases = vec![(wav, 96000)];
        if let Some(path) = std::env::var_os("MITAKA_TEST_MEDIA_VIDEO") {
            cases.push((path.into(), 90));
        }
        for (path, length) in cases {
            let mut w = Worker::launch(Path::new(&exe), File::open(&path).unwrap(), &path).unwrap();
            let wait =
                |w: &mut Worker, stage: &str, timeout: Duration, done: &dyn Fn(&Value) -> bool| {
                    let start = std::time::Instant::now();
                    loop {
                        assert!(w.refresh().unwrap());
                        if done(&w.state) {
                            eprintln!("{stage} completed in {:?}", start.elapsed());
                            break;
                        }
                        assert!(
                            start.elapsed() < timeout,
                            "{stage} timed out: {:?}",
                            w.state
                        );
                        std::thread::sleep(Duration::from_millis(100));
                    }
                };
            // 子プロセスはOSのWebView初期化に先立って接続先を通知する。
            wait(&mut w, "open player", Duration::from_secs(90), &|s| {
                s["ready"].as_u64().is_some_and(|n| n > 0)
            });
            assert_eq!(w.state["frameLength"], length);
            let command_timeout = Duration::from_secs(15);
            w.request(json!({"op":"play"})).unwrap();
            wait(&mut w, "play", command_timeout, &|s| {
                s["playing"] == true && s["seconds"].as_f64().unwrap() > 0.2
            });
            w.request(json!({"op":"pause"})).unwrap();
            wait(&mut w, "pause", command_timeout, &|s| s["playing"] == false);
            w.request(json!({"op":"frame","frame":length/2})).unwrap();
            wait(&mut w, "seek frame", command_timeout, &|s| {
                s["frame"] == length / 2
            });
            assert!((w.state["seconds"].as_f64().unwrap() - 3.).abs() < 1e-4);
        }
    }
}
