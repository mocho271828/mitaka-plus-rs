use super::{
    assets::Files,
    hidden, lua_payload,
    style::{Paint, Styles},
    Block, Cell, Outline, Piece,
};
use eframe::egui;
use scraper::{ElementRef, Node};
use std::sync::Arc;

pub(super) struct Layout<'a> {
    pub blocks: Vec<Block>,
    pub outlines: Vec<Outline>,
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
    inset: egui::epaint::Marginf,
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
            outlines: vec![],
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
    fn begin_block(&mut self, ctx: Context) {
        self.blocks.push(Block {
            align: ctx.style.align.unwrap_or(egui::Align::LEFT),
            inset: ctx.inset,
            heading: ctx.heading,
            cell: ctx.cell,
            parts: vec![],
        });
        self.break_line = false;
    }
    fn push(&mut self, ctx: Context, part: Piece) {
        if self.break_line
            || self
                .blocks
                .last()
                .is_none_or(|b| b.cell != ctx.cell || b.heading != ctx.heading)
        {
            self.begin_block(ctx);
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
                    | "center"
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
        let start = self.blocks.len();
        let outer = ctx.inset;
        let margin = style.borders.margin();
        let outlined = (block || name == "body") && margin != Default::default();
        if outlined {
            ctx.inset = outer + margin;
        }
        if block {
            self.break_line = true;
        }
        if matches!(name, "td" | "th") {
            self.begin_block(ctx);
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
                self.push(ctx, Piece::Image(image, size, style));
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
        if outlined {
            if start == self.blocks.len() {
                self.begin_block(ctx);
            }
            self.outlines.push(Outline {
                blocks: start..self.blocks.len(),
                outer,
                paint: style,
            });
        }
        if block {
            self.break_line = true;
        }
    }
}
