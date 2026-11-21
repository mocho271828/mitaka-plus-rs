//! CF96 の太陽中心J2000赤道座標を黄道座標へ変換し、AUのまま透視投影する。
use eframe::egui::{self, Color32, Pos2, Rect, Stroke};
use mitaka_core::{
    astronomy::j2000_equatorial_to_ecliptic,
    ephemeris::Cf96Ephemeris,
    lunar::{earth_center, lunar_position},
};
use std::{io, time::Instant};

pub const NAMES: [&str; 11] = [
    "太陽",
    "水星",
    "金星",
    "地球",
    "火星",
    "木星",
    "土星",
    "天王星",
    "海王星",
    "冥王星系",
    "月",
];
const COLORS: [Color32; 11] = [
    Color32::GOLD,
    Color32::GRAY,
    Color32::LIGHT_YELLOW,
    Color32::LIGHT_BLUE,
    Color32::from_rgb(230, 118, 81),
    Color32::from_rgb(215, 179, 145),
    Color32::from_rgb(222, 199, 147),
    Color32::from_rgb(139, 200, 209),
    Color32::from_rgb(100, 134, 214),
    Color32::GRAY,
    Color32::LIGHT_GRAY,
];
const PERIODS: [f64; 9] = [
    87.97, 224.70, 365.26, 686.98, 4332.59, 10759.22, 30688.5, 60182., 90560.,
];
const RADII_KM: [f64; 11] = [
    696000., 2439.7, 6051.8, 6378.1366, 3396.19, 71492., 60268., 25559., 24764., 1185., 1738.092,
];
const KM_PER_AU: f64 = 149_597_870.7;
type Point = [f64; 3];
fn plus(a: Point, b: Point) -> Point {
    std::array::from_fn(|i| a[i] + b[i])
}
fn minus(a: Point, b: Point) -> Point {
    std::array::from_fn(|i| a[i] - b[i])
}
fn scale(a: Point, b: f64) -> Point {
    a.map(|v| v * b)
}
fn dot(a: Point, b: Point) -> f64 {
    a.into_iter().zip(b).map(|(x, y)| x * y).sum()
}

fn planet(system: &Cf96Ephemeris, index: usize, jd: f64) -> io::Result<Point> {
    let position = system.state(index, jd)?.position_au;
    let position = if index == 2 {
        earth_center(position, lunar_position(jd)?)
    } else {
        position
    };
    j2000_equatorial_to_ecliptic(position)
}
struct Scene {
    jd: f64,
    bodies: [Point; 11],
    orbits: Vec<Vec<Point>>,
}
impl Scene {
    fn load(system: &Cf96Ephemeris, jd: f64) -> io::Result<Self> {
        let mut bodies = [[0.; 3]; 11];
        let mut orbits = Vec::new();
        for (index, period) in PERIODS.into_iter().enumerate() {
            bodies[index + 1] = planet(system, index, jd)?;
            let [lo, hi] = system.time_range(index)?;
            let (mut lo, mut hi) = ((jd - period / 2.).max(lo), (jd + period / 2.).min(hi));
            if index == 2 {
                lo = lo.max(mitaka_core::lunar::LUNAR_MINIMUM_JD);
                hi = hi.min(mitaka_core::lunar::LUNAR_MAXIMUM_JD);
            }
            let path = (0..=180)
                .map(|n| planet(system, index, lo + (hi - lo) * f64::from(n) / 180.))
                .collect::<io::Result<Vec<_>>>()?;
            orbits.push(path);
        }
        let moon = lunar_position(jd)?;
        bodies[10] = plus(bodies[3], j2000_equatorial_to_ecliptic(moon)?);
        Ok(Self { jd, bodies, orbits })
    }
}
#[derive(Clone, Copy)]
pub struct SolarCamera {
    pub target: Point,
    pub distance: f64,
    pub yaw: f64,
    pub pitch: f64,
    pub roll: f64,
}
impl Default for SolarCamera {
    fn default() -> Self {
        Self {
            target: [0.; 3],
            distance: 100.,
            yaw: 35.,
            pitch: 35.,
            roll: 0.,
        }
    }
}
impl SolarCamera {
    fn basis(self) -> [Point; 3] {
        let (sy, cy) = self.yaw.to_radians().sin_cos();
        let (sp, cp) = self.pitch.to_radians().sin_cos();
        let (sr, cr) = self.roll.to_radians().sin_cos();
        let right = [-sy, cy, 0.];
        let up = [-sp * cy, -sp * sy, cp];
        [
            plus(scale(right, cr), scale(up, sr)),
            minus(scale(up, cr), scale(right, sr)),
            [cp * cy, cp * sy, sp],
        ]
    }
    fn project(self, pos: Point, rect: Rect) -> Option<(Pos2, f64)> {
        let [right, up, out] = self.basis();
        let rel = minus(pos, self.target);
        let depth = self.distance - dot(rel, out);
        if depth <= self.distance * 0.0001 {
            return None;
        }
        let factor = f64::from(rect.height()) / (2. * (22.5_f64).to_radians().tan() * depth);
        let xy = rect.center()
            + egui::vec2(
                (dot(rel, right) * factor) as f32,
                (-dot(rel, up) * factor) as f32,
            );
        xy.is_finite().then_some((xy, depth))
    }
}
struct Flight {
    start: Instant,
    source: SolarCamera,
    duration: f64,
}
pub struct Solar {
    scene: Option<Scene>,
    pub camera: SolarCamera,
    pub selected: usize,
    pub orbits: bool,
    follow: bool,
    flight: Option<Flight>,
    pub show_controls: bool,
}
impl Default for Solar {
    fn default() -> Self {
        Self {
            scene: None,
            camera: Default::default(),
            selected: 0,
            orbits: true,
            follow: false,
            flight: None,
            show_controls: false,
        }
    }
}
impl Solar {
    pub fn prepare(&mut self, system: &Cf96Ephemeris, jd: f64) -> io::Result<()> {
        if self.scene.as_ref().is_none_or(|s| s.jd != jd) {
            self.scene = Some(Scene::load(system, jd)?);
        }
        Ok(())
    }
    pub fn start_flight(&mut self) {
        self.follow = false;
        self.flight = Some(Flight {
            start: Instant::now(),
            source: self.camera,
            duration: 3.,
        });
    }
    pub fn stop(&mut self) {
        self.follow = false;
        self.flight = None;
    }
    pub fn panel(&mut self, ctx: &egui::Context) {
        let mut open = self.show_controls;
        egui::Window::new("太陽系の天体と飛行")
            .open(&mut open)
            .show(ctx, |ui| {
                let before = self.selected;
                egui::ComboBox::from_label("天体")
                    .selected_text(NAMES[self.selected])
                    .show_ui(ui, |ui| {
                        for (i, name) in NAMES.iter().enumerate() {
                            ui.selectable_value(&mut self.selected, i, *name);
                        }
                    });
                if before != self.selected {
                    self.stop();
                }
                ui.horizontal(|ui| {
                    if ui.button("選択天体へ飛行").clicked() {
                        self.start_flight();
                    }
                    if ui.button("飛行・追従を停止").clicked() {
                        self.stop();
                    }
                });
                ui.checkbox(&mut self.orbits, "惑星の軌跡");
                ui.label(format!("カメラ距離 {:.8} AU", self.camera.distance));
            });
        self.show_controls = open;
    }
    pub fn draw(&mut self, ui: &mut egui::Ui, system: &Cf96Ephemeris, jd: f64) -> io::Result<()> {
        self.prepare(system, jd)?;
        let (response, painter) =
            ui.allocate_painter(ui.available_size(), egui::Sense::click_and_drag());
        let rect = response.rect;
        let modifiers = ui.input(|i| i.modifiers);
        if response.dragged() {
            self.stop();
            let d = response.drag_delta();
            if modifiers.shift || response.dragged_by(egui::PointerButton::Secondary) {
                let [r, u, _] = self.camera.basis();
                let factor = 2. * self.camera.distance * (22.5_f64).to_radians().tan()
                    / f64::from(rect.height().max(1.));
                self.camera.target = plus(
                    self.camera.target,
                    scale(
                        minus(scale(u, f64::from(d.y)), scale(r, f64::from(d.x))),
                        factor,
                    ),
                );
            } else if modifiers.alt {
                self.camera.roll = (self.camera.roll + f64::from(d.x) * 0.35) % 360.;
            } else {
                self.camera.yaw = (self.camera.yaw - f64::from(d.x) * 0.35) % 360.;
                self.camera.pitch = (self.camera.pitch + f64::from(d.y) * 0.35).clamp(-89., 89.);
            }
        }
        if response.hovered() {
            let factor = ui.input(|i| {
                (-i.smooth_scroll_delta.y * 0.008).clamp(-2., 2.).exp() / i.zoom_delta()
            });
            if (factor - 1.).abs() > 1e-6 {
                self.flight = None;
                self.camera.distance = (self.camera.distance * f64::from(factor)).clamp(1e-8, 1e12);
            }
        }
        let scene = self
            .scene
            .as_ref()
            .expect("current dates are loaded before use");
        let dest = scene.bodies[self.selected];
        if let Some(flight) = &self.flight {
            let time = (flight.start.elapsed().as_secs_f64() / flight.duration).min(1.);
            let f = (1. - (time * std::f64::consts::PI).cos()) * 0.5;
            self.camera.target = plus(scale(flight.source.target, 1. - f), scale(dest, f));
            let end = RADII_KM[self.selected] / KM_PER_AU * 9.;
            self.camera.distance = ((1. - f) * flight.source.distance.ln() + f * end.ln()).exp();
            if time >= 1. {
                self.flight = None;
                self.follow = true;
            } else {
                ui.ctx().request_repaint();
            }
        } else if self.follow {
            self.camera.target = dest;
        }
        if self.orbits {
            let stroke = Stroke::new(0.7_f32, Color32::from_rgb(59, 77, 100));
            for path in &scene.orbits {
                for segment in path.windows(2) {
                    if let (Some((a, _)), Some((b, _))) = (
                        self.camera.project(segment[0], rect),
                        self.camera.project(segment[1], rect),
                    ) {
                        if let Some([a, b]) = clip_line(rect.expand(1.), a, b) {
                            painter.line_segment([a, b], stroke);
                        }
                    }
                }
            }
        }
        let mut visible: Vec<_> = scene
            .bodies
            .iter()
            .enumerate()
            .filter_map(|(i, p)| {
                self.camera
                    .project(*p, rect)
                    .map(|(pos, depth)| (i, pos, depth))
            })
            .collect();
        visible.sort_by(|a, b| b.2.total_cmp(&a.2));
        let click = response
            .clicked()
            .then(|| response.interact_pointer_pos())
            .flatten();
        let mut pick = None;
        let mut nearest = f32::INFINITY;
        for (i, pos, depth) in visible {
            let radius = (RADII_KM[i] / KM_PER_AU * f64::from(rect.height())
                / (2. * (22.5_f64).to_radians().tan() * depth)) as f32;
            let radius = radius.clamp(
                if i == 0 { 5. } else { 2.6 },
                rect.width().max(rect.height()) * 2.,
            );
            if !rect.expand(radius).contains(pos) {
                continue;
            }
            painter.circle_filled(pos, radius, COLORS[i]);
            if i == self.selected {
                painter.circle_stroke(pos, radius + 3., Stroke::new(1.0_f32, Color32::WHITE));
            }
            painter.text(
                pos + egui::vec2(radius + 5., 0.),
                egui::Align2::LEFT_CENTER,
                NAMES[i],
                egui::FontId::proportional(12.),
                COLORS[i],
            );
            if let Some(click) = click {
                let dist = pos.distance(click);
                if dist <= radius.max(8.) && dist < nearest {
                    pick = Some(i);
                    nearest = dist;
                }
            }
        }
        if let Some(i) = pick {
            self.stop();
            self.selected = i;
            ui.ctx().request_repaint();
        }
        Ok(())
    }
}
/// 広い視野から極端にズームしても、GPUへ巨大な線座標を渡さない。
fn clip_line(rect: Rect, a: Pos2, b: Pos2) -> Option<[Pos2; 2]> {
    let delta = b - a;
    let mut start = 0f32;
    let mut end = 1f32;
    for (p, q) in [
        (-delta.x, a.x - rect.left()),
        (delta.x, rect.right() - a.x),
        (-delta.y, a.y - rect.top()),
        (delta.y, rect.bottom() - a.y),
    ] {
        if p == 0. {
            if q < 0. {
                return None;
            }
        } else if p < 0. {
            start = start.max(q / p);
        } else {
            end = end.min(q / p);
        }
    }
    (start <= end).then_some([a + delta * start, a + delta * end])
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn camera_projects_target_and_culls_behind_eye() {
        let c = SolarCamera::default();
        let rect = Rect::from_min_max(Pos2::ZERO, Pos2::new(640., 480.));
        assert_eq!(c.project(c.target, rect), Some((rect.center(), 100.)));
        assert!(c.project(scale(c.basis()[2], 101.), rect).is_none());
    }
    #[test]
    fn clip_limits_wide_orbit_to_canvas() {
        let r = Rect::from_min_max(Pos2::ZERO, Pos2::new(10., 10.));
        assert_eq!(
            clip_line(r, Pos2::new(-10., 5.), Pos2::new(30., 5.)),
            Some([Pos2::new(0., 5.), Pos2::new(10., 5.)])
        );
    }
}
