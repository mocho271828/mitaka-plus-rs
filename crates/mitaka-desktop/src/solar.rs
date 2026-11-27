//! CF96 の太陽中心J2000赤道座標を黄道座標へ変換し、AUのまま透視投影する。
mod eclipse;
mod scales;
pub(crate) mod sphere;
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
pub(crate) const COLORS: [Color32; 11] = [
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
pub(super) const RADII_KM: [f64; 11] = [
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

pub(crate) fn planet(system: &Cf96Ephemeris, index: usize, jd: f64) -> io::Result<Point> {
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
    jupiter_casters: Vec<mitaka_core::eclipse::Sphere>,
    eclipse_warnings: Vec<String>,
    satellite_positions_checked: bool,
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
        // 月の軌道は現在の地球を基点とした地心軌道として表示する。
        let lo = (jd - 13.67).max(mitaka_core::lunar::LUNAR_MINIMUM_JD);
        let hi = (jd + 13.67).min(mitaka_core::lunar::LUNAR_MAXIMUM_JD);
        let moon_path = (0..=90)
            .map(|i| {
                lunar_position(lo + (hi - lo) * f64::from(i) / 90.)
                    .and_then(j2000_equatorial_to_ecliptic)
                    .map(|p| plus(bodies[3], p))
            })
            .collect::<io::Result<Vec<_>>>()?;
        orbits.push(moon_path);
        Ok(Self {
            jd,
            bodies,
            orbits,
            jupiter_casters: vec![],
            eclipse_warnings: vec![],
            satellite_positions_checked: false,
        })
    }
}
#[derive(Clone, Copy, PartialEq)]
pub struct SolarCamera {
    pub(crate) fov: f64,
    pub(crate) frame: Option<[[f64; 3]; 3]>,
    pub(crate) frame_is_orbit: bool,
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
            fov: 45.,
            frame: None,
            frame_is_orbit: false,
            yaw: 35.,
            pitch: 35.,
            roll: 0.,
        }
    }
}
impl SolarCamera {
    pub(crate) fn basis(self) -> [Point; 3] {
        if let Some(f) = self.frame {
            return f;
        }
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
    pub(crate) fn project(self, pos: Point, rect: Rect) -> Option<(Pos2, f64)> {
        let [right, up, out] = self.basis();
        let rel = minus(pos, self.target);
        let depth = self.distance - dot(rel, out);
        if depth <= self.distance * 0.0001 {
            return None;
        }
        let factor = f64::from(rect.height()) / (2. * (self.fov * 0.5).to_radians().tan() * depth);
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
struct SurfaceFlight {
    body: usize,
    lat: f64,
    lon: f64,
    height_m: f64,
    started: Instant,
    duration: f64,
    source: SolarCamera,
    ascend: bool,
}
struct SurfaceTarget {
    lon: f64,
    lat: f64,
    distance: f64,
}
fn site_target(
    body: usize,
    lat: f64,
    lon: f64,
    height_m: f64,
    jd: f64,
) -> io::Result<SurfaceTarget> {
    let axes = mitaka_core::rotation::planet_orientation(body - 1, jd)?;
    let (sl, cl) = lat.to_radians().sin_cos();
    let (so, co) = lon.to_radians().sin_cos();
    let normal = plus(
        plus(scale(axes[0], cl * co), scale(axes[1], cl * so)),
        scale(axes[2], sl),
    );
    Ok(SurfaceTarget {
        lon: normal[1].atan2(normal[0]).to_degrees(),
        lat: normal[2].clamp(-1., 1.).asin().to_degrees(),
        distance: (RADII_KM[body] + (height_m + 2.) / 1000.) / KM_PER_AU,
    })
}
fn angle_lerp(a: f64, b: f64, f: f64) -> f64 {
    a + ((b - a + 180.).rem_euclid(360.) - 180.) * f
}
pub(crate) struct ViewSettings<'a> {
    pub data: &'a crate::catalog::SkyData,
    pub jd: f64,
    pub picking: crate::viewer::Picking,
    pub layers: &'a crate::layers::LayerOptions,
    pub surface_site: Option<&'a mut crate::camera_control::session::Site>,
    pub observer: [f64; 2],
}
pub struct Solar {
    scene: Option<Scene>,
    pub camera: SolarCamera,
    pub selected: usize,
    pub layers: crate::planet_layers::PlanetLayers,
    pub orbits: bool,
    follow: bool,
    flight: Option<Flight>,
    pub show_controls: bool,
    pub(crate) surfaces: sphere::Surfaces,
    eclipse_source: Option<Result<eclipse::Source, String>>,
    eclipse_overlay: eclipse::Overlay,
    surface_flight: Option<SurfaceFlight>,
    pub landed: Option<usize>,
}
impl Default for Solar {
    fn default() -> Self {
        Self {
            scene: None,
            camera: Default::default(),
            selected: 0,
            layers: Default::default(),
            orbits: true,
            follow: false,
            flight: None,
            show_controls: false,
            surfaces: sphere::Surfaces::new(),
            eclipse_source: None,
            eclipse_overlay: Default::default(),
            surface_flight: None,
            landed: None,
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
    pub fn approach(&mut self, body: usize) -> io::Result<()> {
        let scene = self
            .scene
            .as_ref()
            .ok_or_else(|| io::Error::other("太陽系データがありません"))?;
        self.stop_flight_to_target(body, scene.bodies[body]);
        Ok(())
    }
    fn stop_flight_to_target(&mut self, body: usize, dest: Point) {
        self.stop();
        self.selected = body;
        self.follow = true;
        self.camera.target = dest;
        self.camera.distance = RADII_KM[body] * 6. / KM_PER_AU;
    }
    pub fn begin_surface_flight(
        &mut self,
        body: usize,
        lat: f64,
        lon: f64,
        height_m: f64,
        jd: f64,
        ascend: bool,
    ) -> io::Result<()> {
        let site = site_target(body, lat, lon, height_m, jd)?;
        let scene = self
            .scene
            .as_ref()
            .ok_or_else(|| io::Error::other("太陽系データがありません"))?;
        let center = scene.bodies[body];
        if ascend {
            self.camera = SolarCamera {
                target: center,
                fov: self.camera.fov,
                frame: None,
                frame_is_orbit: false,
                yaw: site.lon,
                pitch: site.lat,
                roll: 0.,
                distance: site.distance,
            };
        } else {
            self.approach(body)?;
        }
        self.camera.frame = None;
        self.camera.frame_is_orbit = false;
        self.selected = body;
        self.follow = false;
        self.flight = None;
        self.landed = None;
        self.surface_flight = Some(SurfaceFlight {
            body,
            lat,
            lon,
            height_m,
            started: Instant::now(),
            duration: 7.1,
            source: self.camera,
            ascend,
        });
        Ok(())
    }
    fn tick_surface(&mut self, jd: f64) -> io::Result<bool> {
        let Some(move_) = &self.surface_flight else {
            return Ok(false);
        };
        let body = move_.body;
        let site = site_target(body, move_.lat, move_.lon, move_.height_m, jd)?;
        let t = if move_.duration == 0. {
            1.
        } else {
            (move_.started.elapsed().as_secs_f64() / move_.duration).min(1.)
        };
        let fraction = (1. - (t * std::f64::consts::PI).cos()) * 0.5;
        let distance = if move_.ascend {
            RADII_KM[body] * 6. / KM_PER_AU
        } else {
            site.distance
        };
        self.camera.distance =
            ((1. - fraction) * move_.source.distance.ln() + fraction * distance.ln()).exp();
        self.camera.target = self.scene.as_ref().unwrap().bodies[body];
        self.camera.yaw = angle_lerp(move_.source.yaw, site.lon, fraction);
        self.camera.pitch = move_.source.pitch + (site.lat - move_.source.pitch) * fraction;
        self.camera.roll = angle_lerp(move_.source.roll, 0., fraction);
        if t >= 1. {
            if !move_.ascend {
                self.landed = Some(body);
            }
            self.surface_flight = None;
            self.follow = true;
        }
        Ok(true)
    }
    pub(crate) fn flight_duration(&mut self, seconds: f64) {
        if let Some(f) = &mut self.surface_flight {
            f.duration = seconds
        }
    }
    pub(crate) fn body_centers(&self) -> io::Result<&[Point; 11]> {
        self.scene
            .as_ref()
            .map(|s| &s.bodies)
            .ok_or_else(|| io::Error::other("太陽系の位置がありません"))
    }
    pub(crate) fn body_point(&self, id: usize) -> Option<[f64; 3]> {
        self.scene.as_ref()?.bodies.get(id).copied()
    }
    pub(crate) fn body_distance(id: usize) -> f64 {
        RADII_KM[id] * 6. / KM_PER_AU
    }
    pub fn start_flight(&mut self) {
        self.stop();
        self.flight = Some(Flight {
            start: Instant::now(),
            source: self.camera,
            duration: 3.,
        });
    }
    pub fn stop(&mut self) {
        self.follow = false;
        self.flight = None;
        self.surface_flight = None;
        self.landed = None;
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
                    if ui.button("近接表示").clicked() {
                        let _ = self.approach(self.selected);
                    }
                    if ui.button("飛行・追従を停止").clicked() {
                        self.stop();
                    }
                });
                self.layers.panel(ui);
                ui.checkbox(&mut self.orbits, "惑星の軌跡");
                ui.checkbox(&mut self.surfaces.clouds, "地球の雲");
                ui.checkbox(&mut self.surfaces.lights, "都市光");
                ui.collapsing("食の影", |ui| {
                    let e = &mut self.surfaces.eclipse;
                    ui.checkbox(&mut e.shadows, "表面の影");
                    ui.checkbox(&mut e.borders, "本影・半影の境界線");
                    ui.checkbox(&mut e.umbra, "本影錐");
                    ui.checkbox(&mut e.penumbra, "半影錐");
                    if let Some(scene) = &self.scene {
                        for warning in &scene.eclipse_warnings {
                            ui.label(warning);
                        }
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("大気");
                    for (mode, label) in [
                        (sphere::Air::None, "なし"),
                        (sphere::Air::Single, "単一散乱"),
                        (sphere::Air::Multiple, "多重散乱"),
                    ] {
                        ui.selectable_value(&mut self.surfaces.air, mode, label);
                    }
                });
                ui.add(
                    egui::DragValue::new(&mut self.surfaces.intensity)
                        .range(0.0..=f32::MAX as f64)
                        .prefix("大気の光量 "),
                );
                for warning in self.surfaces.warnings() {
                    ui.label(warning);
                }
                ui.label(format!("カメラ距離 {:.8} AU", self.camera.distance));
            });
        self.show_controls = open;
    }
    pub fn draw(&mut self, ui: &mut egui::Ui, settings: ViewSettings<'_>) -> io::Result<()> {
        let ViewSettings {
            data,
            jd,
            picking,
            layers,
            surface_site,
            observer,
        } = settings;
        let system = &data.ephemeris;
        let root = &data.root;

        self.prepare(system, jd)?;
        if let Some(scene) = self
            .scene
            .as_mut()
            .filter(|s| !s.satellite_positions_checked)
        {
            let source = self
                .eclipse_source
                .get_or_insert_with(|| eclipse::Source::load(root).map_err(|e| e.to_string()));
            match source {
                Ok(source) => {
                    if let Err(error) = source.apply(scene, system) {
                        scene.eclipse_warnings.push(error.to_string());
                    }
                }
                Err(error) => scene.eclipse_warnings.push(error.clone()),
            }
            scene.satellite_positions_checked = true;
        }
        let (response, painter) =
            ui.allocate_painter(ui.available_size(), egui::Sense::click_and_drag());
        let rect = response.rect;
        let modifiers = ui.input(|i| i.modifiers);
        let on_surface = surface_site
            .as_ref()
            .is_some_and(|s| s.body == self.selected && s.height <= 0.002000001);
        if response.dragged() && on_surface {
            let s = surface_site.unwrap();
            let d = response.drag_delta();
            if modifiers.alt {
                s.roll = (s.roll + f64::from(d.x) * 0.35) % 360.;
            } else {
                s.heading = (s.heading + f64::from(d.x) * 0.35).rem_euclid(360.);
                s.elevation = (s.elevation + f64::from(d.y) * 0.35).clamp(-90., 90.);
            }
            s.pose(
                self.body_point(s.body).unwrap().into(),
                jd,
                &mut self.camera,
            )?;
        } else if response.dragged() {
            self.camera.frame = None;
            self.camera.frame_is_orbit = false;
            self.stop();
            let d = response.drag_delta();
            if modifiers.shift || response.dragged_by(egui::PointerButton::Secondary) {
                let [r, u, _] = self.camera.basis();
                let factor = 2. * self.camera.distance * (self.camera.fov * 0.5).to_radians().tan()
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
                self.stop();
                if on_surface {
                    self.camera.fov = (self.camera.fov * f64::from(factor)).clamp(0.001, 90.);
                } else {
                    self.camera.distance = (self.camera.distance * f64::from(factor))
                        .clamp(5e-6, 648000. / std::f64::consts::PI * 1e40);
                }
            }
        }
        if self.tick_surface(jd)? {
            ui.ctx().request_repaint();
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
        scales::paint(&painter, rect, self.camera, layers, observer, jd);
        if self.orbits {
            let stroke = Stroke::new(0.7_f32, Color32::from_rgb(59, 77, 100));
            for (i, path) in scene.orbits.iter().enumerate() {
                for segment in path.windows(2) {
                    if let (Some((a, _)), Some((b, _))) = (
                        self.camera.project(
                            if i == 9 {
                                plus(
                                    scene.bodies[3],
                                    scale(
                                        minus(segment[0], scene.bodies[3]),
                                        self.layers.moon_orbit_scale(),
                                    ),
                                )
                            } else {
                                segment[0]
                            },
                            rect,
                        ),
                        self.camera.project(
                            if i == 9 {
                                plus(
                                    scene.bodies[3],
                                    scale(
                                        minus(segment[1], scene.bodies[3]),
                                        self.layers.moon_orbit_scale(),
                                    ),
                                )
                            } else {
                                segment[1]
                            },
                            rect,
                        ),
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
                if on_surface && i == self.selected {
                    return None;
                }
                let p = if i == 10 {
                    plus(
                        scene.bodies[3],
                        scale(minus(*p, scene.bodies[3]), self.layers.moon_orbit_scale()),
                    )
                } else {
                    *p
                };
                self.camera
                    .project(p, rect)
                    .map(|(pos, depth)| (i, pos, depth))
            })
            .collect();
        visible.sort_by(|a, b| b.2.total_cmp(&a.2));
        let click = picking.pointer(&response);
        let mut pick = None;
        let mut nearest = f32::INFINITY;
        for (i, pos, depth) in visible {
            let radius = (RADII_KM[i] / KM_PER_AU * f64::from(rect.height())
                / (2. * (self.camera.fov * 0.5).to_radians().tan() * depth))
                as f32;
            let physical = radius;
            let factor = self.layers.scales[usize::from(i == 0)];
            let radius = (radius.max(if i == 0 { 5. } else { 2.6 }) * factor)
                .clamp(1., rect.width().max(rect.height()) * 2.);
            if !rect.expand(radius).contains(pos) {
                continue;
            }
            let tile =
                Rect::from_center_size(pos, egui::Vec2::splat(physical * 2.6 + 4.)).intersect(rect);
            let textured = self.layers.visible
                && physical >= 2.
                && tile.is_positive()
                && self.surfaces.draw(
                    &painter,
                    root,
                    self.camera,
                    scene,
                    i,
                    sphere::Region {
                        viewport: rect,
                        tile,
                    },
                )?;
            if self.layers.visible && !textured {
                painter.circle_filled(pos, radius, COLORS[i]);
            }
            if i == self.selected {
                painter.circle_stroke(pos, radius + 3., Stroke::new(1.0_f32, Color32::WHITE));
            }
            if self.layers.axes && i != 0 {
                let axes = if i == 10 {
                    mitaka_core::lunar::lunar_orientation(jd)?
                } else {
                    mitaka_core::rotation::planet_orientation(i - 1, jd)?
                };
                let arm = scale(axes[2], RADII_KM[i] * 3. / KM_PER_AU);
                let center = scene.bodies[i];
                if let (Some((a, _)), Some((b, _))) = (
                    self.camera.project(minus(center, arm), rect),
                    self.camera.project(plus(center, arm), rect),
                ) {
                    if let Some(line) = clip_line(rect, a, b) {
                        painter.line_segment(line, Stroke::new(1_f32, Color32::LIGHT_GREEN));
                    }
                }
            }
            if self.layers.labels {
                painter.text(
                    pos + egui::vec2(radius + 5., 0.),
                    egui::Align2::LEFT_CENTER,
                    NAMES[i],
                    egui::FontId::proportional(12.),
                    COLORS[i],
                );
            }
            if !self.layers.visible && !self.layers.labels {
                continue;
            }
            if let Some(click) = click {
                let dist = pos.distance(click);
                if dist <= radius.max(8.) && dist < nearest {
                    pick = Some(i);
                    nearest = dist;
                }
            }
        }
        if self.layers.visible {
            if on_surface {
                self.surfaces.draw(
                    &painter,
                    root,
                    self.camera,
                    scene,
                    self.selected,
                    sphere::Region {
                        viewport: rect,
                        tile: rect,
                    },
                )?;
            }
            self.eclipse_overlay
                .draw(&painter, rect, scene, self.camera, self.surfaces.eclipse)?;
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
pub(crate) fn clip_line(rect: Rect, a: Pos2, b: Pos2) -> Option<[Pos2; 2]> {
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
    fn site_matches_reference_radius_and_crosses_angle_seam() {
        let s = site_target(3, 35.68, 139.76, -100., 2451545.).unwrap();
        assert!((s.distance * KM_PER_AU - (RADII_KM[3] - 0.098)).abs() < 1e-9);
        assert_eq!(angle_lerp(179., -179., 0.5), 180.);
    }
    #[test]
    fn surface_flight_reaches_site_and_returns_to_orbit() {
        let jd = 2451545.;
        let mut bodies = [[0.; 3]; 11];
        bodies[3] = [1., 0., 0.];
        let mut solar = Solar {
            scene: Some(Scene {
                jd,
                bodies,
                jupiter_casters: vec![],
                eclipse_warnings: vec![],
                satellite_positions_checked: true,
                orbits: vec![],
            }),
            ..Solar::default()
        };
        for ascending in [false, true] {
            solar
                .begin_surface_flight(3, 35., 139., 8., jd, ascending)
                .unwrap();
            solar.surface_flight.as_mut().unwrap().started =
                Instant::now() - std::time::Duration::from_secs(8);
            assert!(solar.tick_surface(jd).unwrap());
            assert!(!solar.tick_surface(jd).unwrap());
            assert_eq!(solar.camera.target, bodies[3]);
            let radius = if ascending {
                RADII_KM[3] * 6.
            } else {
                RADII_KM[3] + 0.010
            };
            assert!((solar.camera.distance * KM_PER_AU - radius).abs() < 1e-8);
            assert_eq!(solar.landed, (!ascending).then_some(3));
        }
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
