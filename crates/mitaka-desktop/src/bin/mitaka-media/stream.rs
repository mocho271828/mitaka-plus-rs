use http_range::HttpRange;
use std::{
    fs::{File, OpenOptions},
    io::{self, Read},
    net::{Ipv4Addr, SocketAddrV4, TcpListener},
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::Duration,
};
use tiny_http::{Header, Method, Request, Response, Server, StatusCode};
use uuid::Uuid;

const CSP:&str="default-src 'none'; media-src 'self'; style-src 'unsafe-inline'; script-src 'none'; connect-src 'none'; object-src 'none'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'";
fn header(name: &str, value: impl AsRef<[u8]>) -> Header {
    Header::from_bytes(name.as_bytes(), value.as_ref()).expect("controlled response header")
}

fn mime(path: &Path) -> Option<&'static str> {
    Some(
        match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
            "mp4" | "m4v" => "video/mp4",
            "mov" => "video/quicktime",
            "webm" => "video/webm",
            "avi" => "video/x-msvideo",
            "wmv" => "video/x-ms-wmv",
            "mpg" | "mpeg" => "video/mpeg",
            "ogg" | "oga" => "audio/ogg",
            "ogv" => "video/ogg",
            "mp3" => "audio/mpeg",
            "wav" => "audio/wav",
            "m4a" => "audio/mp4",
            "aac" => "audio/aac",
            "flac" => "audio/flac",
            "aif" | "aiff" => "audio/aiff",
            "caf" => "audio/x-caf",
            "wma" => "audio/x-ms-wma",
            _ => return None,
        },
    )
}

/// 一つの File から各ブラウザ要求の位置を独立に読む。メディア内部の参照はファイル名に変換しない。
struct Segment {
    file: Arc<File>,
    offset: u64,
    remaining: u64,
}
impl Read for Segment {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = buf
            .len()
            .min(self.remaining.min(usize::MAX as u64) as usize);
        if n == 0 {
            return Ok(0);
        }
        #[cfg(unix)]
        let read = std::os::unix::fs::FileExt::read_at(&*self.file, &mut buf[..n], self.offset)?;
        #[cfg(windows)]
        let read =
            std::os::windows::fs::FileExt::seek_read(&*self.file, &mut buf[..n], self.offset)?;
        if read == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "selected media changed during playback",
            ));
        }
        self.offset += read as u64;
        self.remaining -= read as u64;
        Ok(read)
    }
}
struct Source {
    file: Arc<File>,
    size: u64,
    mime: &'static str,
    route: String,
    host: String,
}
impl Source {
    fn serve(&self, req: Request) -> io::Result<()> {
        let status = |n| Response::empty(StatusCode(n));
        if req
            .headers()
            .iter()
            .find(|h| h.field.equiv("Host"))
            .is_none_or(|h| h.value.as_str() != self.host)
        {
            return req.respond(status(404));
        }
        if !matches!(req.method(), Method::Get | Method::Head) {
            return req.respond(status(405));
        }
        if req.url() == format!("{}/", self.route) {
            let tag = if self.mime.starts_with("video/") {
                "video"
            } else {
                "audio"
            };
            let body=format!("<!doctype html><html lang='ja'><meta charset='utf-8'><meta name='viewport' content='width=device-width, initial-scale=1'><title>メディア</title><style>html,body{{margin:0;height:100%;background:#141619;color:#fff}}body{{display:grid;place-items:center}}video{{width:100%;height:100%}}audio{{width:min(90%,42rem)}}</style><{tag} controls preload='metadata' src='media'></{tag}></html>");
            return req.respond(
                Response::from_string(body)
                    .with_header(header("Content-Type", "text/html; charset=utf-8"))
                    .with_header(header("Content-Security-Policy", CSP))
                    .with_header(header("Referrer-Policy", "no-referrer"))
                    .with_header(header("Cache-Control", "no-store"))
                    .with_header(header("X-Content-Type-Options", "nosniff")),
            );
        }
        if req.url() != format!("{}/media", self.route) {
            return req.respond(status(404));
        }
        let mut first = 0;
        let mut len = self.size;
        let mut code = StatusCode(200);
        let mut headers = vec![
            header("Content-Type", self.mime),
            header("Accept-Ranges", "bytes"),
            header("Cache-Control", "no-store"),
            header("X-Content-Type-Options", "nosniff"),
        ];
        if let Some(range) = req.headers().iter().find(|h| h.field.equiv("Range")) {
            let ranges = HttpRange::parse(range.value.as_str(), self.size);
            let range = ranges.as_deref().ok().and_then(|rs| match rs {
                [r] if r.length > 0 && r.start < self.size && r.length <= self.size - r.start => {
                    Some(r)
                }
                _ => None,
            });
            let Some(r) = range else {
                return req.respond(
                    status(416)
                        .with_header(header("Content-Range", format!("bytes */{}", self.size))),
                );
            };
            first = r.start;
            len = r.length;
            code = StatusCode(206);
            headers.push(header(
                "Content-Range",
                format!("bytes {}-{}/{}", first, first + len - 1, self.size),
            ));
        }
        let size = usize::try_from(len).map_err(io::Error::other)?;
        req.respond(Response::new(
            code,
            headers,
            Segment {
                file: self.file.clone(),
                offset: first,
                remaining: len,
            },
            Some(size),
            None,
        ))
    }
}
pub struct Player {
    pub url: String,
    stopped: Arc<AtomicBool>,
}
impl Player {
    pub fn open(path: &Path) -> io::Result<Self> {
        let mime = mime(path).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "unsupported media extension")
        })?;
        let mut opts = OpenOptions::new();
        opts.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.custom_flags(libc::O_NONBLOCK);
        }
        let file = opts.open(path)?;
        let meta = file.metadata()?;
        if !meta.is_file() || meta.len() == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "select a nonempty regular media file",
            ));
        }
        let listener = TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0))?;
        let host = listener.local_addr()?.to_string();
        let route = format!("/{}", Uuid::new_v4().simple());
        let url = format!("http://{host}{route}/");
        let server = Arc::new(Server::from_listener(listener, None).map_err(io::Error::other)?);
        let source = Arc::new(Source {
            file: Arc::new(file),
            size: meta.len(),
            mime,
            route,
            host,
        });
        let stopped = Arc::new(AtomicBool::new(false));
        // デコーダーはシーク時に先行リクエストと並行して Range を要求できる。
        for _ in 0..3 {
            let server = server.clone();
            let source = source.clone();
            let flag = stopped.clone();
            thread::spawn(move || {
                while !flag.load(Ordering::Acquire) {
                    match server.recv_timeout(Duration::from_millis(150)) {
                        Ok(Some(req)) => {
                            if let Err(e) = source.serve(req) {
                                if !matches!(
                                    e.kind(),
                                    io::ErrorKind::BrokenPipe | io::ErrorKind::ConnectionReset
                                ) {
                                    eprintln!("media stream: {e}");
                                }
                            }
                        }
                        Ok(None) => {}
                        Err(e) => {
                            eprintln!("media stream: {e}");
                            break;
                        }
                    }
                }
            });
        }
        Ok(Player { url, stopped })
    }
}
impl Drop for Player {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::net::TcpStream;
    fn get(addr: &str, path: &str, range: Option<&str>, host: &str) -> (String, Vec<u8>) {
        let mut stream = TcpStream::connect(addr).unwrap();
        let request = format!(
            "GET {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n{}\r\n",
            range.map(|r| format!("Range: {r}\r\n")).unwrap_or_default()
        );
        stream.write_all(request.as_bytes()).unwrap();
        let mut bytes = vec![];
        stream.read_to_end(&mut bytes).unwrap();
        let split = bytes.windows(4).position(|x| x == b"\r\n\r\n").unwrap();
        (
            String::from_utf8(bytes[..split].to_vec()).unwrap(),
            bytes[split + 4..].to_owned(),
        )
    }
    #[test]
    fn serves_only_selected_file_and_valid_ranges() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("日本 語.wav");
        let bytes = b"RIFF test payload";
        std::fs::write(&path, bytes).unwrap();
        let player = Player::open(&path).unwrap();
        let (host, token) = player
            .url
            .strip_prefix("http://")
            .unwrap()
            .split_once('/')
            .unwrap();
        let media = format!("/{token}media");
        let (head, body) = get(host, &format!("/{token}"), None, host);
        assert!(head.contains("200 OK"));
        assert!(head.contains(CSP));
        assert!(String::from_utf8(body).unwrap().contains("<audio controls"));
        let (head, body) = get(host, &media, None, host);
        assert!(head.contains("200 OK"));
        assert_eq!(body, bytes);
        let (head, body) = get(host, &media, Some("bytes=1-3"), host);
        assert!(head.contains("206"));
        assert_eq!(body, &bytes[1..4]);
        let (head, body) = get(host, &media, Some("bytes=-3"), host);
        assert!(head.contains("206"));
        assert_eq!(body, &bytes[bytes.len() - 3..]);
        assert!(get(host, &media, Some("bytes=10000-"), host)
            .0
            .contains("416"));
        assert!(get(host, "/other/media", None, host).0.contains("404"));
        assert!(get(host, &media, None, "unrelated.local").0.contains("404"));
    }
}
