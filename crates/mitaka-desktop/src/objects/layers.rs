use super::depth::Canvas;
use super::{catalog::Catalog, Kind};
use crate::catalog::SkyData;
use eframe::egui::{self, Align2, Color32, FontId, Pos2, Shape, Stroke};

#[derive(Clone, Copy)]
pub(super) struct Options {
    pub selected_only: bool,
    pub points: bool,
    pub labels: bool,
    pub paths: bool,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            selected_only: false,
            points: true,
            labels: false,
            paths: false,
        }
    }
}

pub(super) struct Overlay<'a> {
    pub catalog: &'a Catalog,
    pub kind: Kind,
    pub active: Option<usize>,
    pub selections: &'a super::selections::Selections,
    pub display: &'a super::display::Display,
    pub sky: &'a SkyData,
    pub jd: f64,
}
impl Overlay<'_> {
    pub fn permits_paths(&self) -> bool {
        if self.kind != Kind::Small || !self.display.groups[..3].iter().any(|o| o.paths) {
            return true;
        }
        let mut counts = [0usize; 2];
        for i in 0..self.catalog.len() {
            let Some(o) = self.display.for_row(self.catalog, i) else {
                continue;
            };
            if !o.paths {
                continue;
            }
            if o.selected_only
                && !self
                    .catalog
                    .identity(i)
                    .is_some_and(|(src, n)| self.selections.contains(self.kind, src, n))
            {
                continue;
            }
            let comet = usize::from(self.catalog.group(i) == Some(2));
            counts[comet] += self.display.sampling.count;
            if counts[comet] > [2097152, 524288][comet] {
                return false;
            }
        }
        true
    }
    pub fn draw(
        &self,
        ctx: &mut Canvas<'_>,
        click: Option<Pos2>,
        allowed_paths: bool,
    ) -> Option<(usize, f32)> {
        if !matches!(self.kind, Kind::Satellites | Kind::Small | Kind::Spacecraft) {
            return None;
        }
        let mut visible = Vec::new();
        let color = if self.kind == Kind::Satellites {
            Color32::from_rgb(223, 205, 152)
        } else {
            Color32::from_rgb(163, 214, 250)
        };
        let path_color = Color32::from_rgba_unmultiplied(163, 214, 250, 89);
        for i in 0..self.catalog.len() {
            let selected = self
                .catalog
                .identity(i)
                .is_some_and(|(src, ordinal)| self.selections.contains(self.kind, src, ordinal));
            let Some(options) = self.display.for_row(self.catalog, i) else {
                continue;
            };
            if options.selected_only && !selected {
                continue;
            }
            if !options.points && !options.labels && !options.paths {
                continue;
            }
            let Some(p) = self.catalog.scene_point(i, self.sky, self.jd) else {
                continue;
            };
            if options.paths && allowed_paths {
                if let Some(samples) = self.catalog.craft_path(i) {
                    ctx.path(samples.iter().map(|v| Some(v.coordinates)), path_color);
                } else if let Some(path) =
                    self.catalog.satellite_path(i, &self.sky.ephemeris, self.jd)
                {
                    ctx.path(path.iter().copied().map(Some), path_color);
                } else if let (Some(record), Some(path)) = (
                    self.catalog.small_record(i),
                    self.catalog.small_path(i, self.display.sampling),
                ) {
                    let curve = super::orbit::Curve {
                        record,
                        camera: ctx.camera,
                        rect: ctx.rect,
                    };
                    curve.visit(&path, self.jd, p, |a, b| {
                        ctx.path([Some(a), Some(b)], path_color)
                    });
                }
            }
            if options.points || options.labels {
                if let Some((pt, d)) = ctx
                    .camera
                    .project(p, ctx.rect)
                    .filter(|(pt, _)| ctx.rect.contains(*pt))
                {
                    visible.push((i, pt, d, selected, options));
                }
            }
        }
        visible.sort_by(|a, b| b.2.total_cmp(&a.2));
        let mut nearest = 8.0;
        let mut hit = None;
        for (i, p, depth, selected, options) in visible {
            if options.points {
                ctx.push(
                    p,
                    depth,
                    Shape::circle_filled(
                        p,
                        if self.active == Some(i) || selected {
                            3.
                        } else {
                            1.7
                        },
                        color,
                    ),
                );
                if self.active == Some(i) {
                    ctx.push(
                        p,
                        depth,
                        Shape::circle_stroke(p, 7., Stroke::new(1.5_f32, Color32::GOLD)),
                    );
                }
                if let Some(d) = click
                    .filter(|&q| ctx.visible(p, depth) && ctx.visible(q, depth))
                    .map(|m| p.distance(m))
                    .filter(|d| *d <= nearest)
                {
                    nearest = d;
                    hit = Some((i, d));
                }
            }
            if options.labels {
                let text = ctx.painter.ctx().fonts(|fonts| {
                    Shape::text(
                        fonts,
                        p + egui::vec2(10., 6.),
                        Align2::LEFT_TOP,
                        self.catalog.title(i),
                        FontId::proportional(11.),
                        color,
                    )
                });
                ctx.push(p, depth, text);
            }
        }
        hit
    }
}
