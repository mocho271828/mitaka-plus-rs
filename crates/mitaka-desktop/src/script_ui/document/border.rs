//! 選択文書のブロック・画像で描く四辺。CSSの枠線は子要素に継承されない。
use eframe::egui::{self, Color32, Rect, Stroke};

#[derive(Clone, Copy, Default)]
pub(super) struct Borders([Option<Edge>; 4]);

#[derive(Clone, Copy, Default)]
struct Edge {
    width: f32,
    color: Option<Color32>,
    dotted: bool,
}
impl Borders {
    pub fn set(&mut self, key: &str, value: &str) -> bool {
        let mut width = None;
        let mut style = None;
        let mut color = None;
        for token in value.split_ascii_whitespace() {
            if matches!(token, "none" | "hidden" | "solid" | "dotted") && style.is_none() {
                style = Some(token);
            } else if let Some(n) = token
                .strip_suffix("px")
                .unwrap_or(token)
                .parse::<f32>()
                .ok()
                .filter(|n| (0.0..=128.).contains(n))
            {
                if width.replace(n).is_some() {
                    return false;
                }
            } else if let Some(c) = super::style::color(token) {
                if color.replace(c).is_some() {
                    return false;
                }
            } else {
                return false;
            }
        }
        let visible = matches!(style, Some("solid" | "dotted"));
        let edge = Some(Edge {
            width: if visible { width.unwrap_or(3.) } else { 0. },
            dotted: style == Some("dotted"),
            color,
        });
        match key {
            "border" => self.0.fill(edge),
            "border-top" => self.0[0] = edge,
            "border-right" => self.0[1] = edge,
            "border-bottom" => self.0[2] = edge,
            "border-left" => self.0[3] = edge,
            _ => return false,
        }
        true
    }

    pub fn apply(self, target: &mut Self) {
        for (to, from) in target.0.iter_mut().zip(self.0) {
            if from.is_some() {
                *to = from;
            }
        }
    }

    pub fn margin(self) -> egui::epaint::Marginf {
        let [top, right, bottom, left] = self.0.map(|e| e.map_or(0., |e| e.width));
        egui::epaint::Marginf {
            top,
            right,
            bottom,
            left,
        }
    }

    pub fn draw(self, ui: &egui::Ui, rect: Rect, fg: Color32) {
        if !ui.is_rect_visible(rect) {
            return;
        }
        for (side, edge) in self.0.iter().enumerate() {
            let Some(edge) = edge.filter(|e| e.width > 0.) else {
                continue;
            };
            let bounds = rect.shrink(edge.width / 2.);
            let endpoints = match side {
                0 => [bounds.left_top(), bounds.right_top()],
                1 => [bounds.right_top(), bounds.right_bottom()],
                2 => [bounds.left_bottom(), bounds.right_bottom()],
                _ => [bounds.left_top(), bounds.left_bottom()],
            };
            let color = edge.color.unwrap_or(fg);
            if edge.dotted {
                // 縦長文書でも画面外の点を際限なく生成しない。
                let clip = ui.clip_rect().expand(edge.width);
                if clip.intersects(Rect::from_two_pos(endpoints[0], endpoints[1])) {
                    let [a, b] = endpoints.map(|p| clip.clamp(p));
                    ui.painter().extend(egui::Shape::dotted_line(
                        &[a, b],
                        color,
                        (2. * edge.width).max(1.),
                        edge.width / 2.,
                    ));
                }
            } else {
                ui.painter()
                    .line_segment(endpoints, Stroke::new(edge.width, color));
            }
        }
    }
}
