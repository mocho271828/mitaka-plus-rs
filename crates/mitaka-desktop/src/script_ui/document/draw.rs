use super::{assets::Files, Block, Outline, Piece, Tag};
use eframe::egui;

pub(super) fn draw(
    ui: &mut egui::Ui,
    blocks: &[Block],
    outlines: &[Outline],
    files: &mut Files,
    trusted: bool,
) -> Option<Tag> {
    let mut selected = None;
    let mut rects = Vec::with_capacity(blocks.len());
    let mut draw_blocks = |ui: &mut egui::Ui, group: &[Block]| {
        for block in group {
            let index = rects.len();
            rects.push(draw_block(ui, block, index, files, trusted, &mut selected));
        }
    };
    let table_id = |b: &Block| b.cell.map(|c| c.table);
    for group in blocks.chunk_by(|a, b| table_id(a) == table_id(b)) {
        let Some(id) = table_id(&group[0]) else {
            draw_blocks(ui, group);
            continue;
        };
        let rows = || group.chunk_by(|a, b| a.cell.unwrap().row == b.cell.unwrap().row);
        let columns = rows()
            .map(|row| row.chunk_by(|a, b| a.cell == b.cell).count())
            .max()
            .unwrap_or(1);
        let gap = 12.;
        let col_width = ((ui.available_width() + gap) / columns as f32 - gap).max(16.);
        egui::Grid::new(("html-table", id))
            .max_col_width(col_width)
            .striped(true)
            .spacing(egui::vec2(gap, 4.))
            .show(ui, |ui| {
                for row in rows() {
                    for cell in row.chunk_by(|a, b| a.cell == b.cell) {
                        ui.vertical(|ui| {
                            ui.set_width(col_width);
                            draw_blocks(ui, cell);
                        });
                    }
                    ui.end_row();
                }
            });
    }
    for outline in outlines {
        let bounds = rects[outline.blocks.clone()]
            .iter()
            .copied()
            .fold(egui::Rect::NOTHING, egui::Rect::union);
        outline.paint.borders.draw(
            ui,
            bounds - outline.outer,
            outline.paint.fg.unwrap_or(egui::Color32::BLACK),
        );
    }
    selected
}
enum Inline<'a> {
    Text(std::sync::Arc<egui::Galley>),
    Link(std::sync::Arc<egui::Galley>, bool),
    Image(usize, egui::Vec2, &'a super::Paint),
}

impl<'a> Inline<'a> {
    fn layout(part: &'a Piece, block: &Block, ui: &egui::Ui, width: f32) -> Self {
        let (rich, link) = match part {
            Piece::Image(i, size, paint) => return Self::Image(*i, *size, paint),
            Piece::Text(text, paint) => (paint.rich(text, block.heading, false), None),
            Piece::Link {
                label,
                style,
                can_run,
                ..
            } => (style.rich(label, block.heading, true), Some(*can_run)),
        };
        let mut job = egui::WidgetText::from(rich).into_layout_job(
            ui.style(),
            egui::FontSelection::Default,
            ui.text_valign(),
        );
        job.wrap.max_width = width;
        job.halign = block.align;
        let galley = ui.fonts(|f| f.layout_job(job));
        match link {
            Some(can_run) => Self::Link(galley, can_run),
            None => Self::Text(galley),
        }
    }

    fn width(&self) -> f32 {
        match self {
            Self::Text(galley) | Self::Link(galley, _) => galley.size().x,
            Self::Image(_, size, paint) => size.x + paint.borders.margin().sum().x,
        }
    }

    fn draw(
        &self,
        ui: &mut egui::Ui,
        files: &mut Files,
        trusted: bool,
        index: (usize, usize),
    ) -> Option<Tag> {
        match self {
            Self::Text(text) => {
                ui.label(text.clone());
            }
            Self::Link(text, can_run) => {
                let response = ui.add_enabled(!can_run || trusted, egui::Link::new(text.clone()));
                let response = if *can_run {
                    response
                } else {
                    response.on_hover_text("このリンクは移動・実行できません")
                };
                let shift = response.rect.min - text.rect.min;
                // 折り返したアンカーの短い行では、余白へのクリックを処理しない。
                if response.clicked()
                    && response.interact_pointer_pos().is_none_or(|pos| {
                        text.rows.iter().any(|row| row.rect.contains(pos - shift))
                    })
                {
                    return Some(if *can_run {
                        Tag::Action(index.0, index.1)
                    } else {
                        Tag::Revoke
                    });
                }
            }
            Self::Image(i, size, paint) => {
                let frame = egui::Frame::NONE
                    .inner_margin(egui::Margin::from(paint.borders.margin()))
                    .show(ui, |ui| files.images[*i].draw(ui, *size));
                paint.borders.draw(
                    ui,
                    frame.response.rect,
                    paint.fg.unwrap_or(egui::Color32::BLACK),
                );
            }
        }
        None
    }
}

fn draw_block(
    ui: &mut egui::Ui,
    block: &Block,
    index: usize,
    files: &mut Files,
    trusted: bool,
    selected: &mut Option<Tag>,
) -> egui::Rect {
    let width = (ui.available_width() - block.inset.sum().x).max(1.);
    egui::Frame::NONE
        .inner_margin(egui::Margin::from(block.inset))
        .show(ui, |ui| {
            ui.set_width(width);
            let spacing = ui.spacing().item_spacing.x;
            let items: Vec<_> = block
                .parts
                .iter()
                .map(|p| Inline::layout(p, block, ui, width))
                .collect();
            let mut start = 0;
            while start < items.len() {
                let mut end = start + 1;
                let mut line = items[start].width().min(width);
                while end < items.len() && line + spacing + items[end].width() <= width {
                    line += spacing + items[end].width();
                    end += 1;
                }
                ui.horizontal(|ui| {
                    let offset = block.align.align_size_within_range(line, 0. ..=width).min;
                    if offset > 0. {
                        ui.add_space(offset);
                    }
                    for (i, item) in items[start..end].iter().enumerate() {
                        if let Some(tag) = item.draw(ui, files, trusted, (index, start + i)) {
                            *selected = Some(tag);
                        }
                    }
                });
                start = end;
            }
        })
        .response
        .rect
}
