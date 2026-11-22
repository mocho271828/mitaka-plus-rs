use crate::{
    catalog::SkyData,
    projection::{Camera, View},
};
use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Stroke};
use mitaka_core::{astronomy::ObservationFrame, catalog::Star};

pub struct Options {
    pub magnitude: f32,
    pub lines: bool,
    pub labels: bool,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            magnitude: 6.5,
            lines: true,
            labels: true,
        }
    }
}
pub struct Chart<'a> {
    pub view: View,
    pub camera: Camera,
    pub frame: &'a ObservationFrame,
    pub rect: Rect,
}
impl Chart<'_> {
    fn point(&self, coord: [f64; 2]) -> Pos2 {
        let [x, y] = self.camera.position(self.view, coord);
        Pos2::new(
            self.rect.left() + x as f32 * self.rect.width(),
            self.rect.top() + y as f32 * self.rect.height(),
        )
    }
    pub fn observed(&self, ra: f32, dec: f32) -> Option<[f64; 2]> {
        if self.view == View::Sky {
            Some([ra.into(), dec.into()])
        } else {
            self.frame.to_horizontal(ra.into(), dec.into()).ok()
        }
    }
    fn star_position(&self, star: &Star) -> Option<(Pos2, f32)> {
        let coords = self.observed(star.ra_deg, star.dec_deg)?;
        let opacity = if self.view == View::Horizon && coords[1] < 0. {
            0.3
        } else {
            1.
        };
        Some((self.point(coords), opacity))
    }
    fn draw_horizon(&self, painter: &egui::Painter) {
        // カメラを傾けると、円筒投影上の地平線が曲線になる。
        let mut mesh = egui::Mesh::default();
        let tilt = self.camera.latitude.to_radians();
        let color = Color32::from_rgb(23, 31, 34);
        for i in 0..=192 {
            let fraction = i as f32 / 192.;
            let longitude = ((f64::from(fraction) - 0.5) * 360. / self.camera.zoom).to_radians();
            let altitude = (-longitude.cos() * tilt.sin())
                .atan2(tilt.cos())
                .to_degrees();
            let y = self.rect.center().y
                - (altitude * self.camera.zoom / 180.) as f32 * self.rect.height();
            let x = self.rect.left() + fraction * self.rect.width();
            mesh.colored_vertex(
                Pos2::new(x, y.clamp(self.rect.top(), self.rect.bottom())),
                color,
            );
            mesh.colored_vertex(Pos2::new(x, self.rect.bottom()), color);
            if i > 0 {
                let k = 2 * i;
                mesh.add_triangle(k - 2, k - 1, k);
                mesh.add_triangle(k, k - 1, k + 1);
            }
        }
        painter.add(egui::Shape::mesh(mesh));
        for (az, name) in [(0., "北"), (90., "東"), (180., "南"), (270., "西")] {
            let p = self.point([az, 0.]);
            if self.rect.contains(p) {
                painter.text(
                    p,
                    Align2::LEFT_TOP,
                    name,
                    FontId::proportional(13.),
                    Color32::LIGHT_GRAY,
                );
            }
        }
    }
    pub fn paint(
        &self,
        painter: &egui::Painter,
        data: &SkyData,
        options: &Options,
        selected: Option<i32>,
        click: Option<Pos2>,
    ) -> Option<i32> {
        if self.view == View::Horizon {
            self.draw_horizon(painter);
        }
        if options.lines || options.labels {
            for constellation in &data.constellations {
                if options.lines {
                    for line in &constellation.hip_lines {
                        let mut previous = None;
                        for hip in line {
                            let p = data
                                .by_hip
                                .get(hip)
                                .and_then(|index| self.star_position(&data.stars[*index]))
                                .map(|(pos, _)| pos);
                            if let (Some(a), Some(b)) = (previous, p) {
                                // 経度0/360の継ぎ目を横断する線だけ、左右に分割して描く。
                                let a: Pos2 = a;
                                let period = self.rect.width() * self.camera.zoom as f32;
                                let stroke = Stroke::new(0.8_f32, Color32::from_rgb(58, 83, 116));
                                if (a.x - b.x).abs() > period / 2. {
                                    let shift =
                                        egui::vec2(if a.x < b.x { -period } else { period }, 0.);
                                    painter.line_segment([a, b + shift], stroke);
                                    painter.line_segment([a - shift, b], stroke);
                                } else {
                                    painter.line_segment([a, b], stroke);
                                }
                            }
                            previous = p;
                        }
                    }
                }
                if options.labels {
                    if let Some(coords) = self.observed(constellation.ra_deg, constellation.dec_deg)
                    {
                        let p = self.point(coords);
                        if self.rect.contains(p) {
                            painter.text(
                                p,
                                Align2::CENTER_CENTER,
                                String::from_utf8_lossy(&constellation.abbreviation),
                                FontId::proportional(11.),
                                Color32::from_rgb(109, 137, 168),
                            );
                        }
                    }
                }
            }
        }
        let mut pick = None;
        let mut min_sq = 8f32.powi(2);
        let mut selected_point = None;
        for star in &data.stars {
            if star.magnitude > options.magnitude {
                continue;
            }
            let Some((p, fade)) = self.star_position(star) else {
                continue;
            };
            if !self.rect.contains(p) {
                continue;
            }
            let warmth = star.bv.clamp(0., 1.6) / 1.6;
            let opacity = (1.2 - star.magnitude * 0.07).clamp(0.3, 1.) * fade;
            let color = Color32::from_rgba_unmultiplied(
                ((0.78 + 0.22 * warmth) * 255.) as u8,
                ((0.87 - 0.12 * warmth) * 255.) as u8,
                ((1. - 0.4 * warmth) * 255.) as u8,
                (opacity * 255.) as u8,
            );
            painter.circle_filled(p, (2.8 - star.magnitude * 0.32).clamp(0.55, 3.6), color);
            if selected == Some(star.hip) {
                selected_point = Some(p);
            }
            if let Some(click) = click {
                let delta = p.distance_sq(click);
                if delta <= min_sq {
                    pick = Some(star.hip);
                    min_sq = delta;
                }
            }
        }
        if let (Some(point), Some(hip)) = (selected_point, selected) {
            painter.circle_stroke(point, 8., Stroke::new(1.2_f32, Color32::GOLD));
            painter.text(
                point + egui::vec2(11., -8.),
                Align2::LEFT_BOTTOM,
                format!("HIP {hip}"),
                FontId::proportional(13.),
                Color32::GOLD,
            );
        }
        pick
    }
}
