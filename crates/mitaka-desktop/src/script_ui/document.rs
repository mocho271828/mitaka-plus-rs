use encoding_rs::SHIFT_JIS;
use same_file::Handle;
use scraper::{ElementRef, Html, Node, Selector};
use sha2::{Digest, Sha256};
use std::{
    ffi::OsStr,
    fs::OpenOptions,
    io::{self, Read},
    path::{Path, PathBuf},
};

const MAX_DOCUMENT: usize = 1024 * 1024;
const MAX_SCRIPT: usize = 64 * 1024;

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

struct Snapshot {
    path: PathBuf,
    handle: Handle,
    digest: [u8; 32],
}

fn read(path: &Path) -> io::Result<(Snapshot, Vec<u8>)> {
    let ext = path.extension().and_then(OsStr::to_str).unwrap_or("");
    if !path.is_absolute() || !["html", "htm"].iter().any(|x| ext.eq_ignore_ascii_case(x)) {
        return Err(invalid("ローカルの HTML ファイルを選択してください"));
    }
    let real = path.canonicalize()?;
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NONBLOCK | libc::O_NOFOLLOW);
    }
    let mut file = options.open(&real)?;
    let before = file.metadata()?;
    if !before.is_file() || before.len() > MAX_DOCUMENT as u64 {
        return Err(invalid("HTML の上限は 1 MiB です"));
    }
    let handle = Handle::from_file(file.try_clone()?)?;
    let mut bytes = Vec::with_capacity(before.len() as usize);
    (&mut file)
        .take(MAX_DOCUMENT as u64 + 1)
        .read_to_end(&mut bytes)?;
    let after = file.metadata()?;
    if bytes.len() != before.len() as usize
        || bytes.len() != after.len() as usize
        || before.modified()? != after.modified()?
        || path.canonicalize()? != real
        || Handle::from_file(options.open(&real)?)? != handle
    {
        return Err(invalid("読み込み中に HTML が変わりました"));
    }
    Ok((
        Snapshot {
            path: real,
            handle,
            digest: Sha256::digest(&bytes).into(),
        },
        bytes,
    ))
}

#[derive(Debug, PartialEq)]
pub enum Piece {
    Text(String),
    Link {
        label: String,
        url: String,
        can_run: bool,
    },
}

#[derive(Default)]
pub struct Block {
    pub heading: bool,
    pub parts: Vec<Piece>,
}

pub struct Document {
    path: PathBuf,
    snapshot: Snapshot,
    pub blocks: Vec<Block>,
    pub trusted: bool,
    pub styles: bool,
}

fn hidden(element: ElementRef<'_>) -> bool {
    let e = element.value();
    e.name.ns.as_ref() != "http://www.w3.org/1999/xhtml"
        || matches!(
            e.name(),
            "head"
                | "script"
                | "style"
                | "template"
                | "iframe"
                | "frame"
                | "object"
                | "embed"
                | "noscript"
                | "canvas"
                | "select"
                | "textarea"
                | "input"
                | "details"
                | "dialog"
        )
        || e.attr("hidden").is_some()
        || e.attr("inert").is_some()
        || e.attr("aria-hidden")
            .is_some_and(|v| v.eq_ignore_ascii_case("true"))
        || e.attr("style").is_some()
}

fn plain_text(element: ElementRef<'_>, depth: usize, output: &mut String) {
    if depth > 128 || hidden(element) {
        return;
    }
    for node in element.children() {
        if let Some(child) = ElementRef::wrap(node) {
            plain_text(child, depth + 1, output);
        } else if let Node::Text(t) = node.value() {
            output.push_str(t);
        }
    }
}

fn line_break(blocks: &mut Vec<Block>) {
    if blocks.last().is_some_and(|b| !b.parts.is_empty()) {
        blocks.push(Block::default());
    }
}

fn collect(e: ElementRef<'_>, depth: usize, blocks: &mut Vec<Block>, allow_links: bool) {
    if depth > 128 || hidden(e) {
        return;
    }
    let name = e.value().name();
    let heading = matches!(name, "h1" | "h2" | "h3" | "h4" | "h5" | "h6");
    let block = heading
        || matches!(
            name,
            "address"
                | "blockquote"
                | "div"
                | "p"
                | "pre"
                | "li"
                | "tr"
                | "dt"
                | "dd"
                | "section"
                | "article"
                | "br"
                | "hr"
        );
    if block {
        line_break(blocks);
    }
    if heading {
        blocks.last_mut().unwrap().heading = true;
    }
    if let ("a", Some(url)) = (name, e.value().attr("href")) {
        let mut text = String::new();
        plain_text(e, depth, &mut text);
        let label = text.split_whitespace().collect::<Vec<_>>().join(" ");
        if !label.is_empty() {
            let target = e.value().attr("target").unwrap_or("");
            let can_run = allow_links
                && (target.is_empty() || target.eq_ignore_ascii_case("_self"))
                && e.value().attr("download").is_none()
                && lua_payload(url).is_ok();
            blocks.last_mut().unwrap().parts.push(Piece::Link {
                label,
                url: url.into(),
                can_run,
            });
        }
        return;
    }
    for child in e.children() {
        if let Some(node) = ElementRef::wrap(child) {
            collect(node, depth + 1, blocks, allow_links);
        } else if let Node::Text(t) = child.value() {
            let text = t.split_whitespace().collect::<Vec<_>>().join(" ");
            if !text.is_empty() {
                blocks.last_mut().unwrap().parts.push(Piece::Text(text));
            }
        }
    }
    if block {
        line_break(blocks);
    }
}

fn lua_payload(url: &str) -> io::Result<String> {
    if !url
        .get(..7)
        .is_some_and(|start| start.eq_ignore_ascii_case("mtkp://"))
    {
        return Err(invalid("Lua のリンクではありません"));
    }
    let input = &url.as_bytes()[7..];
    if input.is_empty() || input.len() > MAX_SCRIPT * 3 {
        return Err(invalid("Lua リンクの長さが不正です"));
    }
    let mut bytes = Vec::new();
    let mut chars = input.iter().copied();
    while let Some(b) = chars.next() {
        let value = if b == b'%' {
            let mut hex = || {
                chars
                    .next()
                    .and_then(|n| char::from(n).to_digit(16))
                    .ok_or_else(|| invalid("不正な % エンコードです"))
            };
            (hex()? * 16 + hex()?) as u8
        } else {
            b
        };
        if value == 0 || bytes.len() == MAX_SCRIPT {
            return Err(invalid("Lua リンクの内容が不正です"));
        }
        bytes.push(value);
    }
    String::from_utf8(bytes).map_err(|_| invalid("Lua リンクは UTF-8 で指定してください"))
}

impl Document {
    pub fn open(path: PathBuf) -> io::Result<Self> {
        let (snapshot, bytes) = read(&path)?;
        let source = std::str::from_utf8(bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&bytes))
            .map(std::borrow::Cow::Borrowed)
            .or_else(|_| {
                SHIFT_JIS
                    .decode_without_bom_handling_and_without_replacement(&bytes)
                    .ok_or_else(|| invalid("HTML を UTF-8 または CP932 として読めません"))
            })?;
        let html = Html::parse_document(&source);
        // 外部・埋め込み CSS や base に依存する文書内では、表示されるリンクの状態を本実装で確定できない。
        let selector = Selector::parse("style, link[rel], base[target], [style]").unwrap();
        let styles = html.select(&selector).any(|e| {
            e.value().name() != "link"
                || e.value().attr("rel").is_some_and(|v| {
                    v.split_ascii_whitespace()
                        .any(|s| s.eq_ignore_ascii_case("stylesheet"))
                })
        });
        let mut blocks = vec![Block::default()];
        collect(html.root_element(), 0, &mut blocks, !styles);
        blocks.retain(|b| !b.parts.is_empty());
        Ok(Self {
            path,
            snapshot,
            blocks,
            trusted: false,
            styles,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// 描画されたアンカーをクリックした時点で選択時のファイルを再照合する。
    pub fn activate(&mut self, block: usize, piece: usize) -> io::Result<String> {
        if !self.trusted {
            return Err(invalid("この文書の Lua 実行を許可してください"));
        }
        let Some(Piece::Link {
            url, can_run: true, ..
        }) = self.blocks.get(block).and_then(|b| b.parts.get(piece))
        else {
            self.trusted = false;
            return Err(invalid("このリンクの実行は許可されていません"));
        };
        match read(&self.path) {
            Ok((now, _))
                if now.path == self.snapshot.path
                    && now.handle == self.snapshot.handle
                    && now.digest == self.snapshot.digest =>
            {
                lua_payload(url)
            }
            _ => {
                self.trusted = false;
                Err(invalid(
                    "HTML が変更されました。ファイルを開き直してください",
                ))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    fn open(html: &str) -> (tempfile::TempDir, Document) {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("試験.html");
        fs::write(&p, html).unwrap();
        (dir, Document::open(p).unwrap())
    }
    #[test]
    fn only_visible_top_level_anchors_are_clickable() {
        let (_dir, mut page) = open(
            r#"<h1>観測</h1><p>日時 <a href="mtkp://print%281%29">実行</a></p>
            <iframe srcdoc="<a href='mtkp://print(2)'>frame</a>"></iframe>
            <a hidden href="mtkp://error()">hidden</a><template><a href="mtkp://error()">template</a></template>
            <a target="_blank" href="mtkp://error()">別窓</a>"#,
        );
        let links: Vec<_> = page
            .blocks
            .iter()
            .flat_map(|p| p.parts.iter())
            .filter(|p| matches!(p, Piece::Link { can_run: true, .. }))
            .collect();
        assert_eq!(links.len(), 1);
        assert!(!page.trusted);
        assert!(page.activate(1, 1).is_err());
        page.trusted = true;
        assert_eq!(page.activate(1, 1).unwrap(), "print(1)");
        fs::write(page.path(), "<p>更新</p>").unwrap();
        assert!(page.activate(1, 1).is_err());
        assert!(!page.trusted);
    }
    #[test]
    fn styled_or_replaced_content_loses_execution_access() {
        let (_dir, p) = open("<style>a{display:none}</style><a href='mtkp://x()'>x</a>");
        assert!(p.styles);
        assert!(matches!(
            p.blocks[0].parts[0],
            Piece::Link { can_run: false, .. }
        ));
        let (_dir, mut p) = open("<a href='mtkp://x()'>x</a>");
        p.trusted = true;
        let previous = p.path().with_extension("old");
        fs::rename(p.path(), &previous).unwrap();
        fs::copy(&previous, p.path()).unwrap();
        assert!(p.activate(0, 0).is_err());
        assert!(!p.trusted);
        assert!(lua_payload("mtkp://%00").is_err());
        assert!(lua_payload("mtkp://%X0").is_err());
        assert_eq!(lua_payload("mtkp://%2520").unwrap(), "%20");
    }
}
