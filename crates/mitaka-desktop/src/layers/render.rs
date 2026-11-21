use super::{
    grid::{self, System},
    LayerOptions,
};
use crate::{catalog::SkyData, chart::Chart, projection::View};
use eframe::egui::{self, Color32, Pos2, Stroke};

impl Chart<'_> {
    fn project_eq(&self, [ra, dec]: [f64; 2]) -> Option<Pos2> {
        let angles = if self.view == View::Sky {
            [ra, dec]
        } else {
            self.frame.to_horizontal(ra, dec).ok()?
        };
        Some(self.point(angles))
    }
    pub(crate) fn stroke_segment(&self, painter: &egui::Painter, a: Pos2, b: Pos2, stroke: Stroke) {
        let emit = |a, b| {
            if self.rect.intersects(egui::Rect::from_two_pos(a, b)) {
                painter.line_segment([a, b], stroke);
            }
        };
        let period = self.rect.width() * self.camera.zoom as f32;
        if (a.x - b.x).abs() > period / 2. {
            let shift = egui::vec2(if a.x < b.x { -period } else { period }, 0.);
            emit(a, b + shift);
            emit(a - shift, b);
        } else {
            emit(a, b);
        }
    }
    pub(crate) fn paint_layers(
        &self,
        painter: &egui::Painter,
        data: &SkyData,
        opts: &LayerOptions,
    ) {
        for (sys, on, rgb) in [
            (System::Equatorial, opts.equatorial, [70, 91, 127]),
            (System::Ecliptic, opts.ecliptic, [150, 116, 61]),
            (System::Galactic, opts.galactic, [102, 83, 123]),
            (System::Horizontal, opts.horizontal, [69, 115, 99]),
        ] {
            if !on {
                continue;
            }
            for line in grid::lines(sys) {
                let mut prev = None;
                for &point in line {
                    let next = if sys == System::Horizontal {
                        if self.view == View::Sky {
                            self.frame
                                .to_equatorial(point[0], point[1])
                                .ok()
                                .map(|p| self.point(p))
                        } else {
                            Some(self.point(point))
                        }
                    } else {
                        self.project_eq(point)
                    };
                    if let (Some(a), Some(b)) = (prev, next) {
                        self.stroke_segment(
                            painter,
                            a,
                            b,
                            Stroke::new(0.75_f32, Color32::from_rgb(rgb[0], rgb[1], rgb[2])),
                        );
                    }
                    prev = next;
                }
            }
        }
        if opts.boundaries {
            for r in &data.constellations {
                if !opts.shows(&r.abbreviation) || r.boundary.len() < 2 {
                    continue;
                }
                for i in 0..r.boundary.len() {
                    let a = r.boundary[i];
                    let b = r.boundary[(i + 1) % r.boundary.len()];
                    let delta = (f64::from(b[0] - a[0]) * 15. + 180.).rem_euclid(360.) - 180.;
                    let steps = (delta.abs().max(f64::from((b[1] - a[1]).abs())) / 2.)
                        .ceil()
                        .clamp(1., 180.) as usize;
                    let mut prev = None;
                    for j in 0..=steps {
                        let k = j as f64 / steps as f64;
                        let next = self.project_eq([
                            f64::from(a[0]) * 15. + delta * k,
                            f64::from(a[1]) + f64::from(b[1] - a[1]) * k,
                        ]);
                        if let (Some(a), Some(b)) = (prev, next) {
                            self.stroke_segment(
                                painter,
                                a,
                                b,
                                Stroke::new(0.7_f32, Color32::from_rgb(100, 85, 54)),
                            );
                        }
                        prev = next;
                    }
                }
            }
        }
        if opts.illustrations {
            let stroke = Stroke::new(0.8_f32, Color32::from_rgb(101, 92, 115));
            for r in &data.layers.art {
                if !opts.shows(&r.abbreviation) {
                    continue;
                }
                let mut prev = None;
                for &idx in &r.art.indices {
                    let next = usize::try_from(idx)
                        .ok()
                        .and_then(|idx| self.project_eq(r.art.coordinates[idx]));
                    if let (Some(a), Some(b)) = (prev, next) {
                        self.stroke_segment(painter, a, b, stroke);
                    }
                    prev = next;
                }
            }
        }
    }
}
