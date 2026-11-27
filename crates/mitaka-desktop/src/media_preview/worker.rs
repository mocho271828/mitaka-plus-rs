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
    const COMMAND_TIMEOUT: Duration = Duration::from_secs(15);

    fn wait(w: &mut Worker, stage: &str, timeout: Duration, done: impl Fn(&Value) -> bool) {
        let start = std::time::Instant::now();
        loop {
            assert!(w.refresh().unwrap(), "{stage}: player closed");
            if done(&w.state) {
                eprintln!("{stage} after {:?}: {}", start.elapsed(), w.state);
                return;
            }
            assert!(start.elapsed() < timeout, "{stage} timed out: {}", w.state);
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    fn seek_paused(w: &mut Worker, frame: u32) {
        w.request(json!({"op":"frame","frame":frame})).unwrap();
        wait(w, "seek frame", COMMAND_TIMEOUT, |s| {
            s["playing"] == false && s["frame"] == frame
        });
    }

    fn play_and_pause(w: &mut Worker, length: u32) {
        let ended = |s: &Value| s["playing"] == false && s["frame"] == length;
        for attempt in 1..=2 {
            w.request(json!({"op":"play"})).unwrap();
            // 巻き戻し後はデコーダーが冒頭を過ぎるまで待ち、実際に途中で停止させる。
            let minimum_time = if attempt == 1 { 0.2 } else { 1. };
            wait(w, "play", COMMAND_TIMEOUT, |s| {
                (s["playing"] == true && s["seconds"].as_f64().unwrap() > minimum_time) || ended(s)
            });
            if !ended(&w.state) {
                w.request(json!({"op":"pause"})).unwrap();
                wait(w, "pause", COMMAND_TIMEOUT, |s| {
                    s["playing"] == false && s["seconds"].as_f64().unwrap() > 0.2
                });
                if !ended(&w.state) {
                    assert!((1..u64::from(length)).contains(&w.state["frame"].as_u64().unwrap()));
                    return;
                }
            }

            // 6秒の媒体が観測の間に自然終了した場合だけ、先頭の停止を確認して再生し直す。
            assert!(
                attempt == 1,
                "playback ended twice before pause: {}",
                w.state
            );
            eprintln!("playback ended before pause. Retrying from frame zero");
            seek_paused(w, 0);
        }
    }

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
        let video = std::env::var_os("MITAKA_TEST_MEDIA_VIDEO")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/seek-15fps.mp4")
            });
        for (path, length) in [(wav, 96000), (video, 90)] {
            eprintln!("testing {} frames from {}", length, path.display());
            let mut w = Worker::launch(Path::new(&exe), File::open(&path).unwrap(), &path).unwrap();
            // 子プロセスはOSのWebView初期化に先立って接続先を通知する。
            wait(&mut w, "open player", Duration::from_secs(90), |s| {
                s["ready"].as_u64().is_some_and(|n| n > 0)
            });
            assert_eq!(w.state["frameLength"], length);
            assert_eq!(w.state["playing"], false);
            assert_eq!(w.state["frame"], 0);
            play_and_pause(&mut w, length);
            // 遅い観測で既に中点で停止した場合も、実際に別フレームからシークする。
            if w.state["frame"] == length / 2 {
                seek_paused(&mut w, 0);
            }
            seek_paused(&mut w, length / 2);
            assert!(
                (w.state["seconds"].as_f64().unwrap() - 3.).abs() < 1e-4,
                "seek result: {}",
                w.state
            );
            eprintln!(
                "seek verified at frame {} of {}, time {} s",
                w.state["frame"], length, w.state["seconds"]
            );
        }
    }
}
