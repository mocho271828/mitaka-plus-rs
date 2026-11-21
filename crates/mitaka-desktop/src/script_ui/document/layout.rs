use super::{
    assets::Files,
    hidden, lua_payload,
    style::{Paint, Styles},
    Block, Cell, Piece, Tag,
};
use eframe::egui;
use scraper::{ElementRef, Node};
use std::sync::Arc;

pub(super) struct Layout<'a> {
    pub blocks: Vec<Block>,
    pub blocked: bool,
    pub paper: egui::Color32,
    styles: &'a Styles,
    files: &'a mut Files,
    nodes: usize,
    break_line: bool,
}
#[derive(Clone, Copy, Default)]
struct Context {
    style: Paint,
    heading: u8,
    cell: Option<Cell>,
}
#[derive(Clone)]
struct Anchor {
    url: Arc<str>,
    active: bool,
}
impl<'a> Layout<'a> {
    pub fn build(root: ElementRef<'_>, styles: &'a Styles, files: &'a mut Files) -> Self {
        let mut out = Self {
            blocks: vec![],
            blocked: styles.blocked,
            paper: egui::Color32::WHITE,
            styles,
            files,
            nodes: 0,
            break_line: true,
        };
        out.walk(
            root,
            0,
            Context {
                style: Paint {
                    fg: Some(egui::Color32::BLACK),
                    bg: Some(egui::Color32::WHITE),
                    ..Default::default()
                },
                ..Default::default()
            },
            None,
        );
        if out.blocked {
            for block in &mut out.blocks {
                for p in &mut block.parts {
                    if let Piece::Link { can_run, .. } = p {
                        *can_run = false;
                    }
                }
            }
        }
        out
    }
    fn push(&mut self, ctx: Context, part: Piece) {
        if self.break_line
            || self
                .blocks
                .last()
                .is_none_or(|b| b.cell != ctx.cell || b.heading != ctx.heading)
        {
            self.blocks.push(Block {
                heading: ctx.heading,
                cell: ctx.cell,
                parts: vec![],
            });
            self.break_line = false;
        }
        self.blocks.last_mut().unwrap().parts.push(part);
    }
    fn text(&mut self, ctx: Context, value: &str, anchor: Option<&Anchor>) {
        let label = value.split_whitespace().collect::<Vec<_>>().join(" ");
        if label.is_empty() {
            return;
        }
        let part = if let Some(anchor) = anchor {
            let can_run = anchor.active && ctx.style.legible();
            Piece::Link {
                label,
                url: Arc::clone(&anchor.url),
                can_run,
                style: ctx.style,
            }
        } else {
            Piece::Text(label, ctx.style)
        };
        self.push(ctx, part)
    }
    fn walk(
        &mut self,
        e: ElementRef<'_>,
        depth: usize,
        mut ctx: Context,
        inherited: Option<&Anchor>,
    ) {
        self.nodes += 1;
        if depth > 128 || self.nodes > 12000 {
            self.blocked = true;
            return;
        }
        if hidden(e) {
            return;
        }
        let el = e.value();
        let name = el.name();
        let (style, safe) = self.styles.paint(e, ctx.style);
        self.blocked |= !safe;
        if [
            "color",
            "bgcolor",
            "background",
            "face",
            "alink",
            "vlink",
            "text",
            "link",
        ]
        .into_iter()
        .any(|k| el.attr(k).is_some())
            || (name == "font" && el.attr("size").is_some())
        {
            self.blocked = true;
        }
        if ["width", "height"]
            .into_iter()
            .filter_map(|k| el.attr(k))
            .any(|s| {
                s.trim_end_matches('%')
                    .parse::<f32>()
                    .is_ok_and(|n| n <= 0.)
            })
        {
            return;
        }
        if style.hidden {
            return;
        }
        ctx.style = style;
        if name == "body" {
            self.paper = style.bg.unwrap_or(egui::Color32::WHITE);
        }
        let key = self.nodes;
        match name {
            "table" => {
                ctx.cell = Some(Cell {
                    table: key,
                    row: key,
                    col: key,
                })
            }
            "tr" => {
                if let Some(c) = &mut ctx.cell {
                    c.row = key;
                    c.col = key;
                }
            }
            "td" | "th" => {
                if let Some(c) = &mut ctx.cell {
                    c.col = key;
                }
            }
            _ => {}
        }
        let heading = match name {
            "h1" => 1,
            "h2" => 2,
            "h3" => 3,
            "h4" => 4,
            "h5" => 5,
            "h6" => 6,
            _ => 0,
        };
        let block = heading != 0
            || matches!(
                name,
                "address"
                    | "blockquote"
                    | "div"
                    | "p"
                    | "pre"
                    | "li"
                    | "tr"
                    | "td"
                    | "th"
                    | "dt"
                    | "dd"
                    | "section"
                    | "article"
                    | "br"
                    | "hr"
                    | "table"
            );
        if block {
            self.break_line = true;
        }
        if matches!(name, "td" | "th") {
            self.blocks.push(Block {
                heading: 0,
                cell: ctx.cell,
                parts: vec![],
            });
            self.break_line = false;
        }
        if heading != 0 {
            ctx.heading = heading;
            ctx.style.bold = true;
        }
        if matches!(name, "strong" | "b" | "th" | "dt") {
            ctx.style.bold = true;
        }
        if matches!(name, "em" | "i") {
            ctx.style.italic = true;
        }
        if name == "u" {
            ctx.style.underline = true;
        }
        if name == "li" {
            self.push(ctx, Piece::Text("•".into(), ctx.style));
        }
        if name == "img" {
            if let Some(image) = el.attr("src").and_then(|src| self.files.image(src)) {
                let original = self.files.images[image].size;
                let dimension = |key: &str| {
                    el.attr(key)
                        .and_then(|v| v.parse::<f32>().ok())
                        .filter(|v| v.is_finite() && *v >= 1. && *v <= 2048.)
                };
                let (w, h) = (dimension("width"), dimension("height"));
                let size = match (w, h) {
                    (Some(w), Some(h)) => egui::vec2(w, h),
                    (Some(w), None) => original * (w / original.x),
                    (None, Some(h)) => original * (h / original.y),
                    _ => original,
                };
                self.push(ctx, Piece::Image(image, size));
            } else if let Some(alt) = el.attr("alt") {
                self.text(ctx, alt, None);
            }
            return;
        }
        let local = el.attr("href").filter(|_| name == "a").map(|s| {
            let target = el.attr("target").unwrap_or("");
            Anchor {
                url: Arc::from(s),
                active: (target.is_empty() || target.eq_ignore_ascii_case("_self"))
                    && el.attr("download").is_none()
                    && lua_payload(s).is_ok(),
            }
        });
        let anchor = local.as_ref().or(inherited);
        for node in e.children() {
            if let Some(child) = ElementRef::wrap(node) {
                self.walk(child, depth + 1, ctx, anchor)
            } else if let Node::Text(t) = node.value() {
                self.text(ctx, t, anchor);
            }
        }
        if block {
            self.break_line = true;
        }
    }
}

pub(super) fn draw(
    ui: &mut egui::Ui,
    blocks: &[Block],
    files: &mut Files,
    trusted: bool,
) -> Option<Tag> {
    let mut selected = None;
    let mut i = 0;
    while i < blocks.len() {
        if let Some(cell) = blocks[i].cell {
            egui::Grid::new(("html-table", cell.table))
                .striped(true)
                .spacing(egui::vec2(12., 4.))
                .show(ui, |ui| {
                    while i < blocks.len() && blocks[i].cell.is_some_and(|c| c.table == cell.table)
                    {
                        let current = blocks[i].cell.unwrap();
                        ui.vertical(|ui| {
                            while i < blocks.len() && blocks[i].cell == Some(current) {
                                draw_block(ui, &blocks[i], i, files, trusted, &mut selected);
                                i += 1;
                            }
                        });
                        if !blocks
                            .get(i)
                            .and_then(|b| b.cell)
                            .is_some_and(|c| c.table == cell.table && c.row == current.row)
                        {
                            ui.end_row();
                        }
                    }
                });
        } else {
            draw_block(ui, &blocks[i], i, files, trusted, &mut selected);
            i += 1;
        }
    }
    selected
}
fn draw_block(
    ui: &mut egui::Ui,
    block: &Block,
    index: usize,
    files: &mut Files,
    trusted: bool,
    selected: &mut Option<Tag>,
) {
    ui.horizontal_wrapped(|ui| {
        for (i, part) in block.parts.iter().enumerate() {
            match part {
                Piece::Text(s, paint) => {
                    ui.label(paint.rich(s, block.heading, false));
                }
                Piece::Link {
                    label,
                    style,
                    can_run,
                    ..
                } => {
                    if *can_run {
                        if ui
                            .add_enabled(
                                trusted,
                                egui::Link::new(style.rich(label, block.heading, true)),
                            )
                            .clicked()
                        {
                            *selected = Some(Tag::Action(index, i));
                        }
                    } else if ui
                        .link(style.rich(label, block.heading, true))
                        .on_hover_text("このリンクは移動・実行できません")
                        .clicked()
                    {
                        *selected = Some(Tag::Revoke);
                    }
                }
                Piece::Image(i, size) => files.images[*i].draw(ui, *size),
            }
        }
    });
}
