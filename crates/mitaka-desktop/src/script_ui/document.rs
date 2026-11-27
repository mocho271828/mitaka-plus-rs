mod assets;
mod layout;
mod style;
use eframe::egui;
use encoding_rs::SHIFT_JIS;
use same_file::Handle;
use scraper::{ElementRef, Html, Selector};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use std::{
    ffi::OsStr,
    fs::OpenOptions,
    io::{self, Read},
    path::{Path, PathBuf},
};
use style::Paint;

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

enum Piece {
    Text(String, Paint),
    Link {
        label: String,
        url: Arc<str>,
        can_run: bool,
        style: Paint,
    },
    Image(usize, egui::Vec2),
}
#[derive(PartialEq, Eq, Clone, Copy)]
struct Cell {
    table: usize,
    row: usize,
    col: usize,
}
struct Block {
    heading: u8,
    cell: Option<Cell>,
    parts: Vec<Piece>,
}
pub(super) enum Tag {
    Action(usize, usize),
    Revoke,
}
pub struct Document {
    path: PathBuf,
    snapshot: Snapshot,
    blocks: Vec<Block>,
    paper: egui::Color32,
    files: assets::Files,
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
                | "button"
                | "datalist"
                | "progress"
                | "marquee"
                | "meter"
                | "video"
                | "audio"
                | "map"
                | "textarea"
                | "input"
                | "details"
                | "dialog"
        )
        || (e.name() == "fieldset" && e.attr("disabled").is_some())
        || e.attr("hidden").is_some()
        || e.attr("inert").is_some()
        || e.attr("aria-hidden")
            .is_some_and(|v| v.eq_ignore_ascii_case("true"))
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

fn decode(bytes: &[u8]) -> io::Result<std::borrow::Cow<'_, str>> {
    std::str::from_utf8(bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes))
        .map(std::borrow::Cow::Borrowed)
        .or_else(|_| {
            SHIFT_JIS
                .decode_without_bom_handling_and_without_replacement(bytes)
                .ok_or_else(|| invalid("文書を UTF-8 または CP932 として読めません"))
        })
}
impl Document {
    pub fn open(path: PathBuf) -> io::Result<Self> {
        let (snapshot, bytes) = read(&path)?;
        let source = decode(&bytes)?;
        let tree = Html::parse_document(&source);
        let mut files = assets::Files::new(
            snapshot
                .path
                .parent()
                .ok_or_else(|| invalid("HTML の親フォルダーがありません"))?,
        );
        let mut styles = style::Styles::default();
        for e in tree.select(&Selector::parse("style,link,base").unwrap()) {
            if e.ancestors().filter_map(ElementRef::wrap).any(|a| {
                matches!(
                    a.value().name(),
                    "template" | "iframe" | "object" | "noscript"
                )
            }) {
                continue;
            }
            if e.value()
                .attr("type")
                .is_some_and(|kind| !kind.is_empty() && !kind.eq_ignore_ascii_case("text/css"))
            {
                continue;
            }
            match e.value().name() {
                "base" => styles.blocked = true,
                "style" => {
                    styles.blocked |= e.value().attr("media").is_some_and(|s| {
                        !s.eq_ignore_ascii_case("all") && !s.eq_ignore_ascii_case("screen")
                    });
                    styles.sheet(&e.text().collect::<String>());
                }
                "link"
                    if e.value().attr("rel").is_some_and(|s| {
                        s.split_ascii_whitespace()
                            .any(|t| t.eq_ignore_ascii_case("stylesheet"))
                    }) =>
                {
                    if e.value().attr("disabled").is_some() {
                        continue;
                    }
                    styles.blocked |= e.value().attr("rel").is_some_and(|s| {
                        s.split_ascii_whitespace()
                            .any(|v| v.eq_ignore_ascii_case("alternate"))
                    }) || e.value().attr("media").is_some_and(|s| {
                        !s.eq_ignore_ascii_case("all") && !s.eq_ignore_ascii_case("screen")
                    });
                    match e.value().attr("href").and_then(|s| files.css(s).ok()) {
                        Some(css) => styles.sheet(&css),
                        None => styles.blocked = true,
                    }
                }
                _ => {}
            }
        }
        styles.finish();
        let layout = layout::Layout::build(tree.root_element(), &styles, &mut files);
        let styles = layout.blocked;
        let paper = layout.paper;
        let blocks = layout.blocks;
        Ok(Self {
            path,
            snapshot,
            blocks,
            files,
            paper,
            trusted: false,
            styles,
        })
    }
    pub(super) fn draw(&mut self, ui: &mut egui::Ui, ready: bool) -> Option<Tag> {
        egui::Frame::new()
            .fill(self.paper)
            .inner_margin(6)
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                layout::draw(ui, &self.blocks, &mut self.files, self.trusted && ready)
            })
            .inner
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
                    && now.digest == self.snapshot.digest
                    && self.files.current() =>
            {
                lua_payload(url)
            }
            _ => {
                self.trusted = false;
                Err(invalid(
                    "HTML または CSS が変更されました。文書を開き直してください",
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
        assert!(p.blocks.is_empty());
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

#[cfg(test)]
mod render_tests {
    use super::*;
    use std::fs;

    fn links(page: &Document) -> Vec<(usize, usize, &str)> {
        page.blocks
            .iter()
            .enumerate()
            .flat_map(|(b, block)| {
                block
                    .parts
                    .iter()
                    .enumerate()
                    .filter_map(move |(p, part)| match part {
                        Piece::Link {
                            label,
                            can_run: true,
                            ..
                        } => Some((b, p, label.as_str())),
                        _ => None,
                    })
            })
            .collect()
    }
    #[test]
    fn css_in_tables_binds_only_a_visible_pointer_click_and_revokes_on_change() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("panel.html");
        let css = dir.path().join("panel.css");
        fs::write(&path,r#"<link rel=stylesheet href=panel.css><body><h2>Catalog</h2><table><tr><th>Name</th><th>Action</th></tr><tr><td></td><td><a href='mtkp://print(42)'>Run</a></td></tr><tr class=gone><td><a href='mtkp://error()'>Hidden</a></td></tr></table><iframe srcdoc="<a href='mtkp://error()'>Frame</a>"></iframe>"#).unwrap();
        fs::write(
            &css,
            "body {color: #fff;background: #234} .gone {display:none} h2 {font-size:24px}",
        )
        .unwrap();
        let mut page = Document::open(path).unwrap();
        assert!(!page.styles && !page.trusted);
        assert_eq!(page.paper, egui::Color32::from_rgb(34, 51, 68));
        assert_eq!(
            links(&page).iter().map(|t| t.2).collect::<Vec<_>>(),
            ["Run"]
        );
        assert_eq!(page.blocks.iter().filter(|b| b.cell.is_some()).count(), 4);
        page.trusted = true;
        let ctx = egui::Context::default();
        let mut interact = None;
        let mut frame = |events| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(500., 250.),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        interact = page.draw(ui, true);
                    });
                },
            )
        };
        frame(vec![]);
        let view = frame(vec![]);
        let point = view
            .shapes
            .iter()
            .find_map(|s| match &s.shape {
                egui::Shape::Text(t) if t.galley.job.text == "Run" => {
                    Some(t.visual_bounding_rect().center())
                }
                _ => None,
            })
            .expect("rendered anchor glyphs");
        for pressed in [true, false] {
            frame(vec![
                egui::Event::PointerMoved(point),
                egui::Event::PointerButton {
                    pos: point,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ]);
        }
        let Some(Tag::Action(b, p)) = interact else {
            panic!("a real click on the label must activate the anchor")
        };
        assert_eq!(page.activate(b, p).unwrap(), "print(42)");
        fs::write(css, "a {display:none}").unwrap();
        assert!(page.activate(b, p).is_err() && !page.trusted);
    }
    #[test]
    fn unresolved_css_and_unverified_visibility_disable_execution() {
        for head in [
            "<link rel=stylesheet href='../a.css'>",
            "<link rel=stylesheet href='https://invalid.example/style.css'>",
            "<style>@media(max-width: 500px){a{display:none}}</style>",
            "<style>a{transform:scale(0)}</style>",
            "<base target='_blank'>",
        ] {
            let d = tempfile::tempdir().unwrap();
            let p = d.path().join("x.html");
            fs::write(&p, format!("{head}<a href='mtkp://error()'>a</a>")).unwrap();
            let page = Document::open(p).unwrap();
            assert!(page.styles && links(&page).is_empty(), "{head}");
        }
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("x.html");
        fs::write(&p,"<body style='background:#000'><div style='opacity:0'><a href='mtkp://x()'>Invisible</a></div><a style='color:#000' href='mtkp://x()'>No contrast</a><span hidden><a href='mtkp://x()'>Hidden</a></span></body>").unwrap();
        assert!(links(&Document::open(p).unwrap()).is_empty());
    }
    #[test]
    #[ignore = "requires the original Plus 1.5.4 ui/index.html"]
    fn original_cp932_styled_panel_keeps_the_five_main_frame_actions() {
        let p = PathBuf::from(std::env::var_os("MITAKA_TEST_HTML").unwrap());
        let page = Document::open(p).unwrap();
        assert!(!page.styles && !page.trusted);
        let labels = links(&page).into_iter().map(|l| l.2).collect::<Vec<_>>();
        assert_eq!(
            labels,
            [
                "日食の様子",
                "時間を進める",
                "時間を戻す",
                "宇宙・地上を切り替え",
                "日食の図解"
            ]
        );
        assert_eq!(page.files.images.len(), 3);
    }
}
