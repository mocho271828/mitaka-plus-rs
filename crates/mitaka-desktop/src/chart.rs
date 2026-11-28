use crate::{
    catalog::SkyData,
    projection::{Camera, View},
};
use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Stroke};
use mitaka_core::{astronomy::ObservationFrame, catalog::Star};

pub struct Options {
    pub layers: crate::layers::LayerOptions,
    pub magnitude: f32,
    pub lines: bool,
    pub labels: bool,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            magnitude: 6.5,
            layers: Default::default(),
            lines: true,
            labels: true,
        }
    }
}
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum ChartTarget {
    Star(i32),
    Body(usize),
}

pub(crate) struct ChartPick<'a> {
    pointer: Pos2,
    opaque: &'a dyn Fn(Pos2) -> bool,
    pub target: Option<ChartTarget>,
    pub distance_sq: f32,
}
impl<'a> ChartPick<'a> {
    pub fn new(pointer: Pos2, opaque: &'a dyn Fn(Pos2) -> bool) -> Self {
        Self {
            pointer,
            opaque,
            target: None,
            distance_sq: f32::INFINITY,
        }
    }
    pub fn consider(&mut self, target: ChartTarget, point: Pos2, radius: f32) {
        let d = point.distance_sq(self.pointer);
        // sky_model.cpp の pick_object は同じ距離の恒星と惑星だけで惑星を優先する。
        let body_tie = d == self.distance_sq
            && matches!(target, ChartTarget::Body(_))
            && matches!(self.target, Some(ChartTarget::Star(_)));
        if d <= radius * radius && (d < self.distance_sq || body_tie) && !(self.opaque)(point) {
            self.target = Some(target);
            self.distance_sq = d;
        }
    }
}
pub struct Chart<'a> {
    pub view: View,
    pub camera: Camera,
    pub frame: &'a ObservationFrame,
    pub rect: Rect,
    pub flight: Option<crate::solar::surface_flight::Overlay>,
}
impl Chart<'_> {
    pub(crate) fn point(&self, coord: [f64; 2]) -> Pos2 {
        let xy = self.camera.position(self.view, coord);
        let [x, y] = self.flight.map_or(xy, |flight| flight.project(xy));
        Pos2::new(
            self.rect.left() + x as f32 * self.rect.width(),
            self.rect.top() + y as f32 * self.rect.height(),
        )
    }
    pub fn observed(&self, ra: f64, dec: f64) -> Option<[f64; 2]> {
        if self.view == View::Sky {
            Some([ra, dec])
        } else {
            self.frame.to_horizontal(ra, dec).ok()
        }
    }
    fn star_position(&self, star: &Star) -> Option<(Pos2, f32)> {
        let coords = self.observed(star.ra_deg.into(), star.dec_deg.into())?;
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
            let sky_x = self.flight.map_or(f64::from(fraction), |f| {
                f.cylinder_uv([f64::from(fraction), 0.5])[0]
            });
            let longitude = ((sky_x - 0.5) * 360. / self.camera.zoom).to_radians();
            let (sr, cr) = self.camera.roll.to_radians().sin_cos();
            let a = longitude.cos() * tilt.sin() + longitude.sin() * sr * tilt.cos();
            let b = tilt.cos() * cr;
            let altitude = if b >= 0. { (-a).atan2(b) } else { a.atan2(-b) }.to_degrees();
            let row = 0.5 - altitude * self.camera.zoom / 180.;
            let screen_y = self.flight.map_or(row, |f| f.project([sky_x, row])[1]);
            let y = self.rect.top() + screen_y as f32 * self.rect.height();
            let x = self.rect.left() + fraction * self.rect.width();
            mesh.colored_vertex(
                Pos2::new(x, y.clamp(self.rect.top(), self.rect.bottom())),
                color,
            );
            mesh.colored_vertex(
                Pos2::new(
                    x,
                    if b >= 0. {
                        self.rect.bottom()
                    } else {
                        self.rect.top()
                    },
                ),
                color,
            );
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
        mut pick: Option<&mut ChartPick<'_>>,
    ) {
        if self.view == View::Horizon {
            self.draw_horizon(painter);
        }
        self.paint_grids(painter, &options.layers);
        if self.view == View::Sky {
            data.deep_sky.borrow_mut().draw(
                &options.layers.deep_sky,
                painter,
                self,
                &data.root,
                options.magnitude,
            );
        }
        self.paint_constellation_layers(painter, data, &options.layers);
        if options.lines || options.labels {
            for constellation in &data.constellations {
                if !options.layers.shows(&constellation.abbreviation) {
                    continue;
                }
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
                                self.stroke_segment(
                                    painter,
                                    a,
                                    b,
                                    Stroke::new(0.8_f32, Color32::from_rgb(58, 83, 116)),
                                );
                            }
                            previous = p;
                        }
                    }
                }
                if options.labels {
                    if let Some(coords) =
                        self.observed(constellation.ra_deg.into(), constellation.dec_deg.into())
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
        let mut selected_point = None;
        if !options.layers.stars {
            return;
        }
        for star in data.stars.iter() {
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
            let (radius, opacity) = options.layers.point_style(star.magnitude);
            let opacity = opacity * fade;
            let color = Color32::from_rgba_unmultiplied(
                ((0.78 + 0.22 * warmth) * 255.) as u8,
                ((0.87 - 0.12 * warmth) * 255.) as u8,
                ((1. - 0.4 * warmth) * 255.) as u8,
                (opacity * 255.) as u8,
            );
            painter.circle_filled(p, radius, color);
            if options.layers.marking == 1 && star.is_double
                || options.layers.marking == 2 && data.layers.hosts.contains(&star.hip)
            {
                let color = if options.layers.marking == 1 {
                    Color32::LIGHT_GREEN
                } else {
                    Color32::YELLOW
                };
                painter.circle_stroke(p, 6., Stroke::new(1.0_f32, color));
            }
            if options.layers.proper_names
                || options.layers.bayer
                || options.layers.flamsteed
                || options.layers.hip
            {
                let name = options.layers.name(&data.layers, star);
                if !name.is_empty() {
                    painter.text(
                        p + egui::vec2(radius + 2., -radius),
                        Align2::LEFT_BOTTOM,
                        name,
                        FontId::proportional(11.),
                        Color32::from_white_alpha((fade * 215.) as u8),
                    );
                }
            }
            if selected == Some(star.hip) {
                selected_point = Some(p);
            }
            if let Some(pick) = pick.as_deref_mut() {
                pick.consider(ChartTarget::Star(star.hip), p, 8.);
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
    }
}
