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
    let mut row_bounds = std::collections::HashMap::new();
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
                    let mut bounds = egui::Rect::NOTHING;
                    for cell in row.chunk_by(|a, b| a.cell == b.cell) {
                        let cell_bounds = ui
                            .vertical(|ui| {
                                ui.set_width(col_width);
                                draw_blocks(ui, cell);
                            })
                            .response
                            .rect;
                        bounds = bounds.union(cell_bounds);
                    }
                    row_bounds.insert(row[0].cell.unwrap().row, bounds.y_range());
                    ui.end_row();
                }
            });
    }
    for outline in outlines {
        let mut bounds = rects[outline.blocks.clone()]
            .iter()
            .copied()
            .fold(egui::Rect::NOTHING, egui::Rect::union);
        if let Some(row) = outline.cell_row.and_then(|id| row_bounds.get(&id)) {
            bounds.min.y = row.min;
            bounds.max.y = row.max;
        }
        // この要素を閉じた後、同じ境界に追加された祖先の枠だけを枠線の外に残す。
        let outer = egui::epaint::Marginf {
            left: outline.outer_left,
            right: outline.outer_right,
            top: blocks[outline.blocks.start].inset.top - outline.top_at_close,
            bottom: blocks[outline.blocks.end - 1].inset.bottom - outline.bottom_at_close,
        };
        outline.paint.borders.draw(
            ui,
            bounds - outer,
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
    fn layout(
        part: &'a Piece,
        block: &Block,
        ui: &egui::Ui,
        width: f32,
        indent: f32,
        mut add: impl FnMut(bool, Self),
    ) {
        let (rich, link) = match part {
            Piece::Image(i, size, paint) => {
                let image = Self::Image(*i, *size, paint);
                add(indent > 0. && indent + image.width() > width, image);
                return;
            }
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
        let item = |galley| match link {
            Some(can_run) => Self::Link(galley, can_run),
            None => Self::Text(galley),
        };
        if block.parts.len() == 1 {
            job.halign = block.align;
            add(false, item(ui.fonts(|fonts| fonts.layout_job(job))));
            return;
        }
        // egui の水平折り返しと同様に、先行する要素の幅を先頭行だけに適用する。
        job.sections[0].leading_space = indent;
        let galley = ui.fonts(|fonts| fonts.layout_job(job));
        if indent == 0. && galley.rows.len() == 1 {
            add(false, item(galley));
            return;
        }
        let mut add_rows = |new_line, rows: &[egui::epaint::text::Row]| {
            if rows.is_empty() || rows[0].glyphs.is_empty() {
                return;
            }
            let mut text = String::new();
            for (i, row) in rows.iter().enumerate() {
                if i > 0 {
                    text.push('\n');
                }
                text.extend(row.glyphs.iter().map(|g| g.chr));
            }
            let mut job =
                egui::text::LayoutJob::single_section(text, galley.job.sections[0].format.clone());
            job.halign = block.align;
            add(new_line, item(ui.fonts(|fonts| fonts.layout_job(job))));
        };
        // 隣の要素と行を共有する両端だけを分け、長文の中間行は一つのウィジェットで描く。
        let last = galley.rows.len() - 1;
        let first = usize::from(indent > 0. && last > 0);
        add_rows(false, &galley.rows[..first]);
        add_rows(first > 0, &galley.rows[first..last]);
        add_rows(last > 0, &galley.rows[last..]);
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
            let mut lines = Vec::new();
            let mut line = Vec::new();
            let mut used = 0.;
            for (i, part) in block.parts.iter().enumerate() {
                let indent = if line.is_empty() { 0. } else { used + spacing };
                Inline::layout(part, block, ui, width, indent, |new_row, item| {
                    if new_row && !line.is_empty() {
                        lines.push((used, std::mem::take(&mut line)));
                        used = 0.;
                    }
                    if !line.is_empty() {
                        used += spacing;
                    }
                    used += item.width();
                    line.push((i, item));
                });
            }
            if !line.is_empty() {
                lines.push((used, line));
            }
            ui.spacing_mut().item_spacing.y = 0.;
            for (used, line) in lines {
                ui.horizontal(|ui| {
                    let offset = block.align.align_size_within_range(used, 0. ..=width).min;
                    if offset > 0. {
                        ui.add_space(offset);
                    }
                    for (i, item) in line {
                        if let Some(tag) = item.draw(ui, files, trusted, (index, i)) {
                            *selected = Some(tag);
                        }
                    }
                });
            }
        })
        .response
        .rect
}
