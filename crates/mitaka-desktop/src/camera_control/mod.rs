mod galaxy_targets;
mod journey;
mod journey_panel;
mod navigation;
mod orientation;
mod places;
pub(crate) mod session;
mod target;
use crate::{commands::Command, projection::View, App};
use eframe::egui;
use glam::DQuat;
use serde_json::{json, Value};
use std::time::Instant;

#[derive(Clone, Copy)]
struct Interval {
    start: Instant,
    seconds: f64,
    smooth: bool,
}
impl Interval {
    fn value(self, now: Instant) -> (f64, bool) {
        let t = if self.seconds == 0. {
            1.
        } else {
            (now.saturating_duration_since(self.start).as_secs_f64() / self.seconds).min(1.)
        };
        (
            if self.smooth {
                (1. - (t * std::f64::consts::PI).cos()) * 0.5
            } else {
                t
            },
            t >= 1.,
        )
    }
}
#[derive(Clone, Copy)]
struct Scalar {
    time: Interval,
    a: f64,
    b: f64,
    log: bool,
}
impl Scalar {
    fn value(self, t: f64) -> f64 {
        if t <= 0. {
            self.a
        } else if t >= 1. {
            self.b
        } else if self.log {
            ((1. - t) * self.a.ln() + t * self.b.ln()).exp()
        } else {
            (1. - t) * self.a + t * self.b
        }
    }
}
enum Rotation {
    Quaternion(DQuat, DQuat),
    Euler([f64; 3], [f64; 3]),
    Add(DQuat, [f64; 3]),
}
impl Rotation {
    fn sample(&self, t: f64) -> DQuat {
        match self {
            Self::Quaternion(a, b) => a.slerp(*b, t),
            Self::Euler(a, b) => {
                orientation::from_euler(std::array::from_fn(|i| a[i] * (1. - t) + b[i] * t))
            }
            Self::Add(q, e) => *q * orientation::from_euler(e.map(|v| v * t)),
        }
    }
}
struct Turn {
    view: View,
    time: Interval,
    rot: Rotation,
}
pub(crate) struct State {
    pub controls: bool,
    entry: String,
    duration: f64,
    pub(crate) mode_duration: f64,
    zoom_mode: u8,
    az: Option<Scalar>,
    alt: Option<Scalar>,
    zoom: Option<Scalar>,
    fov: Option<Scalar>,
    horizon_fov: Option<(f64, f64)>,
    turn: Option<Turn>,
    target: Option<target::Tracking>,
    place: Option<places::Move>,
    place_name: Option<String>,
    journey: journey_panel::Panel,
    physical_flight: Option<navigation::Flight>,
    free_physical: bool,
    pub(crate) site: Option<session::Site>,
}
impl Default for State {
    fn default() -> Self {
        Self {
            controls: false,
            entry: "Planet.Earth".into(),
            duration: 9.,
            mode_duration: 7.1,
            zoom_mode: 2,
            az: None,
            alt: None,
            zoom: None,
            fov: None,
            horizon_fov: None,
            turn: None,
            target: None,
            place: None,
            place_name: None,
            journey: Default::default(),
            physical_flight: None,
            free_physical: false,
            site: None,
        }
    }
}
fn tick(slot: &mut Option<Scalar>, now: Instant) -> Option<f64> {
    let s = (*slot)?;
    let (t, done) = s.time.value(now);
    if done {
        *slot = None
    }
    Some(s.value(t))
}
fn scalar(v: &Value, key: &str) -> Result<f64, String> {
    v[key]
        .as_f64()
        .filter(|n| n.is_finite())
        .ok_or_else(|| format!("{key} は有限数で指定してください"))
}
fn signed(a: f64) -> f64 {
    let n = a % 360.;
    if n > 180. {
        n - 360.
    } else if n < -180. {
        n + 360.
    } else {
        n
    }
}
impl App {
    fn orientation(&self) -> Result<DQuat, String> {
        match self.view {
            View::Solar => Ok(orientation::solar(self.solar.camera)),
            View::Horizon => Ok(orientation::horizon(
                self.horizon,
                orientation::site(self.latitude, self.longitude, self.jd)
                    .map_err(|e| e.to_string())?,
            )),
            _ => Err("この視点ではカメラ姿勢を取得できません".into()),
        }
    }
    fn set_orientation(&mut self, q: DQuat) {
        match self.view {
            View::Solar => {
                if let Some(s) = self
                    .motion
                    .site
                    .as_mut()
                    .filter(|s| s.height <= 0.002000001)
                {
                    if let Ok(local) = orientation::body_site(s.body, s.lat, s.lon, self.jd) {
                        let mut camera = crate::projection::Camera {
                            longitude: s.heading,
                            latitude: s.elevation,
                            roll: s.roll,
                            ..Default::default()
                        };
                        orientation::apply_horizon(&mut camera, local, q);
                        s.heading = camera.longitude;
                        s.elevation = camera.latitude;
                        s.roll = camera.roll;
                        s.last = None;
                        if let Some(center) = self.solar.body_point(s.body) {
                            let _ = s.pose(center.into(), self.jd, &mut self.solar.camera);
                        }
                    }
                } else {
                    orientation::apply_solar(&mut self.solar.camera, q);
                }
            }
            View::Horizon => {
                if let Ok(local) = orientation::site(self.latitude, self.longitude, self.jd) {
                    orientation::apply_horizon(&mut self.horizon, local, q)
                }
            }
            _ => {}
        }
    }
    pub(crate) fn horizon_fov(&self) -> f64 {
        self.motion
            .horizon_fov
            .filter(|(zoom, _)| *zoom == self.horizon.zoom)
            .map_or(180. / self.horizon.zoom, |(_, f)| f)
    }
    pub(crate) fn set_horizon_fov(&mut self, f: f64) {
        self.horizon.zoom = 180. / f.max(1e-9);
        self.motion.horizon_fov = Some((self.horizon.zoom, f));
    }
    pub(crate) fn camera_snapshot(&self, mut msg: Value) -> Value {
        let q = self.orientation().ok();
        let fov = match self.view {
            View::Horizon => Some(self.horizon_fov()),
            View::Solar if self.motion.site.is_some() => Some(self.solar.camera.fov),
            View::Solar if self.solar.selected == 3 => Some(self.horizon_fov()),
            _ => None,
        };
        let site = self
            .motion
            .site
            .filter(|s| self.view == View::Solar && s.height <= 0.002000001);
        let state = json!({"azimuth":signed(site.map_or(self.horizon.longitude,|s|s.heading)),"altitude":site.map_or(self.horizon.latitude,|s|s.elevation),
             "distance":self.solar.camera.distance,"cameraDuration":self.motion.duration,"cameraModeDuration":self.motion.mode_duration,
             "cameraMode":match self.view{View::Horizon|View::Surface=>Some(0),View::Solar=>Some(if self.motion.site.as_ref().is_some_and(|s|s.height<=0.002000001){0}else{1}),_=>None},
             "cameraZoomMode":self.motion.zoom_mode,"cameraFov":fov,"cameraOrientation":q.map(|q|q.to_array()),
             "cameraEuler":q.map(orientation::euler),"target":self.motion.target.as_ref().map(|t|t.name.as_str()).or(self.motion.place_name.as_deref()).unwrap_or(target::NAMES[self.solar.selected])});
        msg.as_object_mut()
            .unwrap()
            .extend(state.as_object().unwrap().clone());
        msg
    }
    pub(crate) fn camera_frame(&mut self, ctx: &egui::Context) {
        let now = Instant::now();
        if let Err(e) = self.tick_journey(now) {
            self.error = Some(e.to_string());
        }
        if let Err(e) = self.tick_place(now) {
            self.error = Some(e.to_string());
        }
        if let Err(e) = self.tick_reference_flight(now) {
            self.error = Some(e.to_string());
        }
        if let Some(mut site) = self.motion.site.take() {
            if let Err(e) = site.advance(self) {
                self.error = Some(e.to_string());
            } else {
                self.motion.site = Some(site);
            }
        }
        if let Some(s) = self
            .motion
            .site
            .as_mut()
            .filter(|s| self.view == View::Solar && s.height <= 0.002000001)
        {
            if let Some(v) = tick(&mut self.motion.az, now) {
                s.heading = v.rem_euclid(360.);
                s.last = None;
            }
            if let Some(v) = tick(&mut self.motion.alt, now) {
                s.elevation = v;
                s.last = None;
            }
            if s.last.is_none() {
                if let Some(center) = self.solar.body_point(s.body) {
                    if let Err(e) = s.pose(center.into(), self.jd, &mut self.solar.camera) {
                        self.error = Some(e.to_string());
                    }
                    s.last = Some(self.jd);
                }
            }
        } else if self.view == View::Horizon {
            if let Some(v) = tick(&mut self.motion.az, now) {
                self.horizon.longitude = v.rem_euclid(360.)
            }
            if let Some(v) = tick(&mut self.motion.alt, now) {
                self.horizon.latitude = v
            }
        } else {
            self.motion.az = None;
            self.motion.alt = None
        }
        if self.view == View::Solar {
            if let Some(d) = tick(&mut self.motion.zoom, now) {
                self.solar.camera.distance = d
            }
        } else {
            self.motion.zoom = None
        }
        if self.view == View::Horizon || (self.view == View::Solar && self.motion.site.is_some()) {
            if let Some(v) = tick(&mut self.motion.fov, now) {
                if self.view == View::Horizon {
                    self.set_horizon_fov(v)
                } else {
                    self.solar.camera.fov = v
                }
            }
        } else {
            self.motion.fov = None
        }
        if let Some(turn) = &self.motion.turn {
            if self.view == turn.view {
                let (t, done) = turn.time.value(now);
                let q = turn.rot.sample(t);
                self.set_orientation(q);
                if done {
                    self.motion.turn = None;
                }
            } else {
                self.motion.turn = None;
            }
        }
        if let Some(mut tr) = self.motion.target.take() {
            if tr.refresh(self, now) {
                self.motion.target = Some(tr);
            }
        }
        let m = &self.motion;
        if m.place.is_some()
            || m.journey.flight.is_some()
            || m.physical_flight.is_some()
            || m.az.is_some()
            || m.alt.is_some()
            || m.zoom.is_some()
            || m.fov.is_some()
            || m.turn.is_some()
            || m.target.as_ref().is_some_and(|t| t.moving(now))
        {
            ctx.request_repaint_after(std::time::Duration::from_millis(16));
        }
    }
    pub(crate) fn camera_action(
        &mut self,
        kind: u64,
        msg: &Value,
        ctx: &egui::Context,
    ) -> Result<(), String> {
        if self.data.is_none() {
            return Err("天文データを開いてください".into());
        }
        let time = Interval {
            start: Instant::now(),
            seconds: scalar(msg, "duration")?,
            smooth: msg["enabled"].as_bool().unwrap_or(false),
        };
        if time.seconds < 0. {
            return Err("補間時間は0秒以上です".into());
        }
        let n = scalar(msg, "number")?;
        self.camera_frame(ctx);
        match kind {
            91 | 101 => {
                if n < 0. {
                    return Err("補間時間は0秒以上です".into());
                }
                if kind == 91 {
                    self.motion.duration = n
                } else {
                    self.motion.mode_duration = n
                }
            }
            102 if [0., 1., 2.].contains(&n) => self.motion.zoom_mode = n as u8,
            12 | 89 | 92 | 93 => {
                let az = matches!(kind, 12 | 92);
                if !az && !(-90. ..=90.).contains(&n) {
                    return Err("仰角は−90から90度です".into());
                }
                let site = self
                    .motion
                    .site
                    .filter(|s| self.view == View::Solar && s.height <= 0.002000001);
                if site.is_none() {
                    self.view = View::Horizon;
                }
                self.motion.turn = None;
                self.motion.target = None;
                let a = if az {
                    site.map_or(self.horizon.longitude, |s| s.heading)
                } else {
                    site.map_or(self.horizon.latitude, |s| s.elevation)
                };
                let b = if az { a + signed(n - a) } else { n };
                let anim = Some(Scalar {
                    a,
                    b,
                    time,
                    log: false,
                });
                if az {
                    self.motion.az = anim
                } else {
                    self.motion.alt = anim
                }
            }
            4 | 103 => {
                if self.view != View::Solar || n <= 0. {
                    return Err("宇宙視点の正の距離を指定してください".into());
                }
                self.motion.zoom = Some(Scalar {
                    a: self.solar.camera.distance,
                    b: n.clamp(0.000005, 648000. / std::f64::consts::PI * 1e9),
                    time: Interval {
                        smooth: false,
                        ..time
                    },
                    log: kind == 103 && time.smooth,
                });
            }
            104 | 105 => {
                if !(n > 0. && n <= 90.) {
                    return Err("画角は0度より大きく90度以下です".into());
                }
                let a = if self.view == View::Horizon {
                    self.horizon_fov()
                } else if self.view == View::Solar && self.motion.site.is_some() {
                    self.solar.camera.fov
                } else {
                    return Err("画角の変更にはプラネタリウム視点が必要です".into());
                };
                self.motion.fov = Some(Scalar {
                    a,
                    b: n,
                    time: Interval {
                        smooth: false,
                        ..time
                    },
                    log: false,
                });
            }
            94..=99 => {
                let current = self.orientation()?;
                let a = msg["orientation"]
                    .as_array()
                    .ok_or("姿勢がありません")?
                    .iter()
                    .map(|v| {
                        v.as_f64()
                            .filter(|v| v.is_finite())
                            .ok_or("姿勢は有限数です")
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let rot = if matches!(kind, 94 | 97) {
                    Rotation::Quaternion(
                        current,
                        orientation::unit(a.try_into().map_err(|_| "四元数には4成分必要です")?)?,
                    )
                } else {
                    let e = a.try_into().map_err(|_| "Euler角には3成分必要です")?;
                    if matches!(kind, 96 | 99) {
                        Rotation::Add(current, e)
                    } else {
                        Rotation::Euler(orientation::euler(current), e)
                    }
                };
                self.motion.az = None;
                self.motion.alt = None;
                self.motion.turn = Some(Turn {
                    view: self.view,
                    time,
                    rot,
                });
            }
            3 | 90 => self.target_action(
                msg["text"].as_str().ok_or("対象名がありません")?,
                kind == 90,
                time,
            )?,
            100 => {
                if ![0., 1.].contains(&n) || time.smooth && time.seconds != 0. {
                    return Err("カメラモードまたは時間が不正です".into());
                }
                if self.view == View::Sky && !time.smooth {
                    return Err("全天図ではモードを切り替えられません".into());
                }
                let ascend = n == 1.;
                if self.view == View::Solar && self.solar.selected != 3 && self.solar.selected != 4
                {
                    self.nonterrain_mode(ascend, time.seconds, time.smooth)
                        .map_err(|e| e.to_string())?;
                } else if (ascend && self.view != View::Solar)
                    || (!ascend && self.view != View::Horizon)
                {
                    if self.view == View::Sky {
                        self.view = if ascend { View::Horizon } else { View::Solar };
                        self.solar.selected = 3;
                    }
                    if self.view == View::Solar && !matches!(self.solar.selected, 3 | 4) {
                        return Err("このモードの切替は地球で利用できます".into());
                    }
                    self.error = None;
                    self.execute(Command::Land, ctx);
                    if let Some(e) = self.error.take() {
                        return Err(e);
                    }
                    self.solar.flight_duration(time.seconds);
                }
            }
            _ => return Err("未対応のカメラ操作です".into()),
        }
        self.camera_frame(ctx);
        ctx.request_repaint();
        Ok(())
    }
}

impl App {
    pub(crate) fn camera_controls(&mut self, ctx: &egui::Context) {
        if !self.motion.controls {
            return;
        }
        let mut open = true;
        egui::Window::new("カメラ").open(&mut open).show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("補間時間");
                ui.add(
                    egui::DragValue::new(&mut self.motion.duration)
                        .range(0. ..=3600.)
                        .suffix(" 秒"),
                );
            });
            if self.view == View::Solar {
                let c = &mut self.solar.camera;
                let mut changed = false;
                ui.horizontal(|ui| {
                    ui.label("方位");
                    changed |= ui
                        .add(egui::DragValue::new(&mut c.yaw).speed(0.25).suffix("°"))
                        .changed();
                });
                ui.horizontal(|ui| {
                    ui.label("仰角");
                    changed |= ui
                        .add(
                            egui::DragValue::new(&mut c.pitch)
                                .range(-89. ..=89.)
                                .speed(0.25)
                                .suffix("°"),
                        )
                        .changed();
                });
                ui.horizontal(|ui| {
                    ui.label("ロール");
                    changed |= ui
                        .add(egui::DragValue::new(&mut c.roll).speed(0.25).suffix("°"))
                        .changed();
                });
                ui.horizontal(|ui| {
                    ui.label("距離");
                    changed |= ui
                        .add(
                            egui::DragValue::new(&mut c.distance)
                                .range(0.000005..=2.06264806247e45)
                                .max_decimals(10)
                                .suffix(" AU"),
                        )
                        .changed();
                });
                ui.horizontal(|ui| {
                    ui.label("垂直画角");
                    changed |= ui
                        .add(
                            egui::DragValue::new(&mut c.fov)
                                .range(0.001..=90.)
                                .speed(0.1)
                                .suffix("°"),
                        )
                        .changed();
                });
                if changed {
                    c.frame = None;
                    c.frame_is_orbit = false;
                    self.motion.turn = None;
                    self.motion.zoom = None;
                    self.motion.site = None;
                }
            } else {
                ui.horizontal(|ui| {
                    ui.label("方位");
                    ui.add(egui::DragValue::new(&mut self.horizon.longitude).suffix("°"));
                    ui.label("仰角");
                    ui.add(
                        egui::DragValue::new(&mut self.horizon.latitude)
                            .range(-90. ..=90.)
                            .suffix("°"),
                    );
                });
                ui.horizontal(|ui| {
                    ui.label("ロール");
                    ui.add(egui::DragValue::new(&mut self.horizon.roll).suffix("°"));
                });
            }
            ui.horizontal(|ui| {
                ui.label("対象");
                ui.text_edit_singleline(&mut self.motion.entry);
            });
            ui.horizontal(|ui| {
                for (label, look) in [("対象へ移動", false), ("地平図で見る", true)] {
                    if ui.button(label).clicked() {
                        let target = self.motion.entry.clone();
                        let t = Interval {
                            start: Instant::now(),
                            seconds: self.motion.duration,
                            smooth: true,
                        };
                        if let Err(e) = self.target_action(&target, look, t) {
                            self.error = Some(e)
                        } else {
                            ctx.request_repaint()
                        }
                    }
                }
            });
        });
        self.motion.controls = open;
    }
}

#[cfg(test)]
mod motion_tests {
    use super::*;
    #[test]
    fn duration_endpoints_and_logarithmic_retarget() {
        let start = Instant::now();
        let time = Interval {
            start,
            seconds: 4.,
            smooth: false,
        };
        let log = Scalar {
            time,
            a: 1.,
            b: 16.,
            log: true,
        };
        let half = start + std::time::Duration::from_secs(2);
        assert!((log.value(time.value(half).0) - 4.).abs() < 1e-12);
        let mut next = Some(Scalar {
            time: Interval {
                start: half,
                seconds: 2.,
                smooth: true,
            },
            a: 4.,
            b: 10.,
            log: false,
        });
        assert_eq!(tick(&mut next, half), Some(4.));
        assert_eq!(
            tick(&mut next, half + std::time::Duration::from_secs(3)),
            Some(10.)
        );
        assert!(next.is_none());
        assert_eq!(signed(359. - 1.), -2.);
    }
}
