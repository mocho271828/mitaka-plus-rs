mod assets;
mod border;
mod draw;
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
    Image(usize, egui::Vec2, Paint),
}
#[derive(PartialEq, Eq, Clone, Copy)]
struct Cell {
    table: usize,
    row: usize,
    col: usize,
}
struct Outline {
    blocks: std::ops::Range<usize>,
    paint: Paint,
    // 左右は祖先の枠幅を、上下はこの要素を閉じた時点で境界に積んだ枠幅を使う。
    outer_left: f32,
    outer_right: f32,
    top_at_close: f32,
    bottom_at_close: f32,
    cell_row: Option<usize>,
}
struct Block {
    align: egui::Align,
    inset: egui::epaint::Marginf,
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
    outlines: Vec<Outline>,
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
        let outlines = layout.outlines;
        Ok(Self {
            path,
            snapshot,
            blocks,
            outlines,
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
                draw::draw(
                    ui,
                    &self.blocks,
                    &self.outlines,
                    &mut self.files,
                    self.trusted && ready,
                )
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
        render(&mut page, &ctx, vec![]);
        let (view, _) = render(&mut page, &ctx, vec![]);
        let point = label_rect(&view, "Run").center();
        let Some(Tag::Action(b, p)) = click(&mut page, &ctx, point) else {
            panic!("a real click on the label must activate the anchor");
        };
        assert_eq!(page.activate(b, p).unwrap(), "print(42)");
        fs::write(css, "a {display:none}").unwrap();
        assert!(page.activate(b, p).is_err() && !page.trusted);
    }
    fn render(
        page: &mut Document,
        ctx: &egui::Context,
        events: Vec<egui::Event>,
    ) -> (egui::FullOutput, Option<Tag>) {
        let mut tag = None;
        let out = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(500., 540.),
                )),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| tag = page.draw(ui, true));
            },
        );
        (out, tag)
    }
    fn label_rect(view: &egui::FullOutput, name: &str) -> egui::Rect {
        view.shapes
            .iter()
            .find_map(|s| match &s.shape {
                egui::Shape::Text(t) if t.galley.job.text == name => Some(t.visual_bounding_rect()),
                _ => None,
            })
            .unwrap()
    }

    fn click(page: &mut Document, ctx: &egui::Context, pos: egui::Pos2) -> Option<Tag> {
        let mut result = None;
        for pressed in [true, false] {
            result = render(
                page,
                ctx,
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            )
            .1;
        }
        result
    }
    #[test]
    fn aligns_inline_content_and_table_cells_with_click_targets_inside_the_borders() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("aligned.html");
        image::RgbaImage::from_pixel(16, 12, image::Rgba([250, 25, 0, 255]))
            .save(d.path().join("test.png"))
            .unwrap();
        fs::write(&p, r#"<style>
           body {text-align:center} div {border:2px solid red}
           h2 {border-left:6px solid blue; border-bottom:2px dotted green}
           td {border:1px solid blue} .right {text-align:right}
           </style><body><div><p align=left>Left</p><h2>Center</h2>
           <p class=right>Label <a href='mtkp://print(42)'>Run</a></p>
           <table><tr><td align=left>LeftCell</td><td>MidCell</td><td class=right>RightCell</td></tr></table>
           <img src="test.png" style="border:2px solid #ff00ff">
           <p style="text-align:right"><a href="mtkp://print(2)">A long linked paragraph that will wrap across a few lines with a short final word to test the empty clickable area</a></p></div>"#).unwrap();
        let mut page = Document::open(p).unwrap();
        assert!(!page.styles);
        let ctx = egui::Context::default();
        render(&mut page, &ctx, vec![]);
        let (view, _) = render(&mut page, &ctx, vec![]);
        let left = label_rect(&view, "Left");
        let center = label_rect(&view, "Center");
        let link = label_rect(&view, "Run");
        assert!(
            left.left() < 35. && (center.center().x - 250.).abs() < 5. && link.right() > 463.,
            "{left:?} {center:?} {link:?}"
        );
        assert!(label_rect(&view, "Label").right() < link.left());
        assert!(label_rect(&view, "LeftCell").left() < 35.);
        assert!((label_rect(&view, "MidCell").center().x - 250.).abs() < 5.);
        assert!(label_rect(&view, "RightCell").right() > 463.);
        let strokes: Vec<_> = view
            .shapes
            .iter()
            .filter_map(|s| match &s.shape {
                egui::Shape::LineSegment { stroke, .. } => Some(stroke),
                _ => None,
            })
            .collect();
        assert_eq!(
            strokes
                .iter()
                .filter(|s| s.color == egui::Color32::RED)
                .count(),
            4
        );
        assert!(strokes
            .iter()
            .any(|s| s.width == 6. && s.color == egui::Color32::BLUE));
        let pink = egui::Color32::from_rgb(255, 0, 255);
        let image_border: Vec<_> = view
            .shapes
            .iter()
            .filter_map(|s| match &s.shape {
                egui::Shape::LineSegment { points, stroke } if stroke.color == pink => Some(points),
                _ => None,
            })
            .collect();
        assert_eq!(image_border.len(), 4);
        assert!((image_border[0][1].x - image_border[0][0].x - 18.).abs() < 0.1);
        let dots = view.shapes.iter().filter(|s| {
            matches!(&s.shape, egui::Shape::Circle(c) if c.fill==egui::Color32::from_rgb(0,128,0))
        }).count();
        assert!(dots > 5);
        assert!(click(&mut page, &ctx, link.center()).is_none());
        page.trusted = true;
        render(&mut page, &ctx, vec![]);
        assert!(click(
            &mut page,
            &ctx,
            egui::pos2(left.center().x, link.center().y)
        )
        .is_none());
        let wrapped = view
            .shapes
            .iter()
            .find_map(|s| match &s.shape {
                egui::Shape::Text(text) if text.galley.job.text.starts_with("A long linked") => {
                    Some(text)
                }
                _ => None,
            })
            .unwrap();
        assert!(wrapped.galley.rows.len() > 1);
        let last = wrapped
            .galley
            .rows
            .last()
            .unwrap()
            .rect
            .translate(wrapped.pos.to_vec2());
        let blank = egui::pos2(wrapped.visual_bounding_rect().left() + 2., last.center().y);
        assert!(blank.x < last.left());
        assert!(click(&mut page, &ctx, blank).is_none());
        let Some(Tag::Action(b, p)) = click(&mut page, &ctx, last.center()) else {
            panic!("wrapped link did not receive the click")
        };
        assert_eq!(page.activate(b, p).unwrap(), "print(2)");
        let Some(Tag::Action(b, p)) = click(&mut page, &ctx, link.center()) else {
            panic!("aligned link did not receive the click")
        };
        assert_eq!(page.activate(b, p).unwrap(), "print(42)");
    }
    #[test]
    fn container_borders_do_not_insert_space_between_paragraphs() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("paragraphs.html");
        let gap = |style| {
            fs::write(
                &path,
                format!("<div style='{style}'><p>One</p><p>Two</p></div>"),
            )
            .unwrap();
            let mut page = Document::open(path.clone()).unwrap();
            let ctx = egui::Context::default();
            let (view, _) = render(&mut page, &ctx, vec![]);
            label_rect(&view, "Two").top() - label_rect(&view, "One").bottom()
        };
        assert_eq!(gap("border:8px solid red"), gap(""));
    }
    #[test]
    fn table_cell_borders_cover_the_whole_row_without_stretching_child_borders() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rows.html");
        fs::write(&path, r#"<table><tr>
            <td style="border-left:2px solid red"><p style="border-left:2px solid black">One</p></td>
            <td style="border-left:2px solid blue">First<br>Second<br>Third</td>
            </tr></table>"#).unwrap();
        let mut page = Document::open(path).unwrap();
        let ctx = egui::Context::default();
        render(&mut page, &ctx, vec![]);
        let (view, _) = render(&mut page, &ctx, vec![]);
        let extent = |color| {
            view.shapes
                .iter()
                .find_map(|entry| match &entry.shape {
                    egui::Shape::LineSegment { points, stroke }
                        if stroke.width == 2. && stroke.color == color =>
                    {
                        Some(egui::emath::Rangef::new(points[0].y, points[1].y))
                    }
                    _ => None,
                })
                .unwrap()
        };
        let left = extent(egui::Color32::RED);
        let right = extent(egui::Color32::BLUE);
        assert_eq!(
            left, right,
            "borders of both cells in this row must share their vertical extent"
        );
        assert!(extent(egui::Color32::BLACK).span() < left.span());
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
        let mut page = Document::open(p).unwrap();
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
        let ctx = egui::Context::default();
        render(&mut page, &ctx, vec![]);
        let (view, _) = render(&mut page, &ctx, vec![]);
        let has_heading_border = view.shapes.iter().any(|s| match &s.shape {
            egui::Shape::LineSegment { stroke, .. } => {
                stroke.width == 6. && stroke.color == egui::Color32::from_gray(102)
            }
            _ => false,
        });
        assert!(has_heading_border);
    }
}
