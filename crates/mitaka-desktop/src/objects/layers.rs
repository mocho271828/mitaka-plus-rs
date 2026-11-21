use super::{catalog::Catalog, Kind, Position};
use crate::{
    catalog::SkyData,
    solar::{clip_line, SolarCamera},
};
use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Stroke};

pub(super) struct Options {
    pub points: bool,
    pub labels: bool,
    pub paths: bool,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            points: true,
            labels: false,
            paths: false,
        }
    }
}

struct Painter<'a> {
    paint: &'a egui::Painter,
    camera: SolarCamera,
    rect: Rect,
}
impl Painter<'_> {
    fn path(&self, samples: impl IntoIterator<Item = Option<[f64; 3]>>, color: Color32) {
        let stroke = Stroke::new(0.8_f32, color);
        let mut previous = None;
        for sample in samples {
            let screen = sample
                .and_then(|p| self.camera.project(p, self.rect))
                .map(|(p, _)| p);
            if let Some([a, b]) = previous
                .zip(screen)
                .and_then(|(a, b)| clip_line(self.rect, a, b))
            {
                self.paint.line_segment([a, b], stroke);
            }
            previous = screen;
        }
    }
}
pub(super) struct Overlay<'a> {
    pub catalog: &'a Catalog,
    pub kind: Kind,
    pub active: Option<usize>,
    pub options: &'a Options,
    pub sky: &'a SkyData,
    pub jd: f64,
}
impl Overlay<'_> {
    pub fn draw(
        &self,
        paint: &egui::Painter,
        camera: SolarCamera,
        rect: Rect,
        click: Option<Pos2>,
    ) -> Option<usize> {
        if !matches!(self.kind, Kind::Satellites | Kind::Small | Kind::Spacecraft) {
            return None;
        }
        let ctx = Painter {
            paint,
            camera,
            rect,
        };
        let mut visible = Vec::new();
        let color = if self.kind == Kind::Satellites {
            Color32::from_rgb(223, 205, 152)
        } else {
            Color32::from_rgb(163, 214, 250)
        };
        let path_color = Color32::from_rgba_unmultiplied(163, 214, 250, 89);
        for i in 0..self.catalog.len() {
            if self.kind == Kind::Small && self.active != Some(i) {
                continue;
            }
            let Ok(Position::Solar(p, _)) = self.catalog.locate(i, self.sky, self.jd) else {
                continue;
            };
            if self.options.paths {
                if let Some(samples) = self.catalog.craft_path(i) {
                    ctx.path(samples.iter().map(|v| Some(v.coordinates)), path_color);
                } else if let Some(path) = self.catalog.small_path(i, self.jd) {
                    ctx.path(path, path_color);
                }
            }
            if self.options.points || self.options.labels {
                if let Some((pt, d)) = camera.project(p, rect).filter(|(pt, _)| rect.contains(*pt))
                {
                    visible.push((i, pt, d));
                }
            }
        }
        visible.sort_by(|a, b| b.2.total_cmp(&a.2));
        let mut nearest = 8.0;
        let mut hit = None;
        for (i, p, _) in visible {
            if self.options.points {
                paint.circle_filled(p, if self.active == Some(i) { 3. } else { 1.7 }, color);
                if self.active == Some(i) {
                    paint.circle_stroke(p, 7., Stroke::new(1.5_f32, Color32::GOLD));
                }
                if let Some(d) = click.map(|m| p.distance(m)).filter(|d| *d <= nearest) {
                    nearest = d;
                    hit = Some(i);
                }
            }
            if self.options.labels {
                paint.text(
                    p + egui::vec2(10., 6.),
                    Align2::LEFT_TOP,
                    self.catalog.title(i),
                    FontId::proportional(11.),
                    color,
                );
            }
        }
        hit
    }
}
