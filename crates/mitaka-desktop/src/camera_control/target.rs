use super::{signed, Interval};
use crate::{catalog::SkyData, objects::ScriptTarget, projection::View, solar::Solar, App};
use glam::DVec3;
use mitaka_core::astronomy::{
    equatorial_coordinates, equatorial_direction, j2000_ecliptic_to_equatorial,
    j2000_equatorial_to_ecliptic, ObservationFrame,
};
use std::{io, time::Instant};
pub(super) const NAMES: [&str; 11] = [
    "Sun",
    "Planet.Mercury",
    "Planet.Venus",
    "Planet.Earth",
    "Planet.Mars",
    "Planet.Jupiter",
    "Planet.Saturn",
    "Planet.Uranus",
    "Planet.Neptune",
    "Planet.Pluto",
    "Satellite.Moon",
];
enum Object {
    Body(usize),
    Hip(i32),
    Galaxy(super::galaxy_targets::Target),
    Direction([f64; 2]),
    Catalog(Box<ScriptTarget>),
}
impl Object {
    fn location(&self, sky: &SkyData, solar: &Solar, jd: f64) -> io::Result<[f64; 3]> {
        match self {
            Self::Body(id) => solar
                .body_point(*id)
                .ok_or_else(|| io::Error::other("太陽系の位置がありません")),
            Self::Galaxy(target) => Ok(target.pos),
            Self::Direction(_) => Err(io::Error::other("この対象には方向のみ指定されています")),
            Self::Hip(hip) => {
                let s = &sky.stars[sky.by_hip[hip]];
                if !s.parallax_mas.is_finite() || s.parallax_mas <= 0. {
                    return Err(io::Error::other("恒星の年周視差がありません"));
                }
                let dir = j2000_equatorial_to_ecliptic(equatorial_direction(
                    s.ra_deg.into(),
                    s.dec_deg.into(),
                )?)?;
                Ok(dir.map(|v| {
                    v * (1000. / f64::from(s.parallax_mas)) * (648000. / std::f64::consts::PI)
                }))
            }
            Self::Catalog(c) => c.position(sky, jd),
        }
    }
    fn horizontal(&self, app: &App) -> io::Result<[f64; 2]> {
        let sky = app
            .data
            .as_ref()
            .ok_or_else(|| io::Error::other("天文データがありません"))?;
        let eq = if let Self::Direction(coords) = self {
            *coords
        } else if let Self::Hip(hip) = self {
            let s = &sky.stars[sky.by_hip[hip]];
            [s.ra_deg.into(), s.dec_deg.into()]
        } else {
            let p = self.location(sky, &app.solar, app.jd)?;
            let earth = app
                .solar
                .body_point(3)
                .ok_or_else(|| io::Error::other("地球の位置がありません"))?;
            equatorial_coordinates(j2000_ecliptic_to_equatorial(std::array::from_fn(|i| {
                p[i] - earth[i]
            }))?)?
        };
        ObservationFrame::new(app.latitude, app.longitude, app.jd)?.to_horizontal(eq[0], eq[1])
    }
}
pub(super) struct Tracking {
    pub name: String,
    marker: Option<[f64; 3]>,
    object: Object,
    view: View,
    time: Interval,
    from: [f64; 3],
    dist: [f64; 2],
    aim: [f64; 2],
    last: Option<[f64; 3]>,
    follow: bool,
}
impl Tracking {
    pub fn moving(&self, now: Instant) -> bool {
        !self.time.value(now).1
    }
    pub fn refresh(&mut self, app: &mut App, now: Instant) -> bool {
        if self.view != app.view {
            return false;
        }
        let (t, done) = self.time.value(now);
        let epoch = [app.jd, app.latitude, app.longitude];
        if done && self.last == Some(epoch) {
            return self.follow;
        }
        let mut apply = || -> io::Result<()> {
            let data = app
                .data
                .as_ref()
                .ok_or_else(|| io::Error::other("天文データがありません"))?;
            app.solar.prepare(&data.ephemeris, app.jd)?;
            if self.view == View::Solar {
                let p = self.object.location(data, &app.solar, app.jd)?;
                self.marker = Some(p);
                app.solar.camera.target = DVec3::from_array(self.from).lerp(p.into(), t).to_array();
                if self.last.is_none() {
                    app.solar.camera.distance = self.dist[0] * (1. - t) + self.dist[1] * t;
                }
            } else {
                let [az, alt] = self.object.horizontal(app)?;
                app.horizon.longitude =
                    (self.aim[0] + signed(az - self.aim[0]) * t).rem_euclid(360.);
                app.horizon.latitude = self.aim[1] * (1. - t) + alt * t;
            }
            Ok(())
        };
        if let Err(e) = apply() {
            app.error = Some(e.to_string());
            return false;
        }
        if done {
            self.last = Some(epoch);
        }
        !done || self.follow
    }
}
impl App {
    pub(super) fn target_action(
        &mut self,
        name: &str,
        look: bool,
        time: Interval,
    ) -> Result<(), String> {
        let sky = self.data.as_ref().ok_or("天文データがありません")?;
        self.solar
            .prepare(&sky.ephemeris, self.jd)
            .map_err(|e| e.to_string())?;
        let object = if let Some(id) = NAMES.iter().position(|u| *u == name) {
            Object::Body(id)
        } else if name.starts_with("Other.")
            || name.starts_with("Galaxy.")
            || name.starts_with("GC.")
        {
            Object::Galaxy(super::galaxy_targets::resolve(name, sky).map_err(|e| e.to_string())?)
        } else if let Some(key) = name.strip_prefix("Constel.") {
            let mut hits = sky
                .constellations
                .iter()
                .filter(|c| c.english_name == key.as_bytes() || c.abbreviation == key.as_bytes());
            let target = hits.next().ok_or("星座がありません")?;
            if hits.next().is_some() {
                return Err("同名の星座が複数あります".into());
            }
            Object::Direction([target.ra_deg.into(), target.dec_deg.into()])
        } else if let Some(id) = name.strip_prefix("Star.") {
            let hip = if let Ok(hip) = id.parse::<i32>() {
                hip
            } else {
                let q = mitaka_core::extended_catalog::load_star_names(
                    &sky.root.join("star_names_HIP.dat"),
                )
                .map_err(|e| e.to_string())?;
                q.iter()
                    .find(|r| r.name_bytes == id.as_bytes())
                    .ok_or("恒星名が見つかりません")?
                    .hip
            };
            if !sky.by_hip.contains_key(&hip) {
                return Err("HIP 番号が見つかりません".into());
            }
            Object::Hip(hip)
        } else {
            Object::Catalog(Box::new(
                ScriptTarget::open(name, sky, &self.objects).map_err(|e| e.to_string())?,
            ))
        };
        if look {
            object.horizontal(self).map_err(|e| e.to_string())?;
        } else {
            object
                .location(sky, &self.solar, self.jd)
                .map_err(|e| e.to_string())?;
        }
        let from = self.solar.camera.target;
        let d = self.solar.camera.distance;
        let mut dest = d;
        if !look {
            self.solar.stop();
            self.motion.site = None;
            if let Object::Body(id) = object {
                let prev = Solar::body_distance(self.solar.selected);
                let next = Solar::body_distance(id);
                if self.motion.zoom_mode == 2 || self.motion.zoom_mode == 1 && next > prev {
                    dest = (next * 1.5).max(0.000005);
                }
                self.solar.selected = id;
            }
            if let Object::Galaxy(t) = object {
                if let Some(r) = t.radius.filter(|r| {
                    self.motion.zoom_mode == 2
                        || self.motion.zoom_mode == 1
                            && *r > Solar::body_distance(self.solar.selected) / 6.
                }) {
                    dest = 9. * r;
                }
            }
            if let Object::Hip(id) = object {
                self.selected = Some(id);
            }
        }
        self.view = if look { View::Horizon } else { View::Solar };
        self.motion.az = None;
        self.motion.alt = None;
        self.motion.turn = None;
        self.motion.target = Some(Tracking {
            name: name.to_owned(),
            marker: None,
            object,
            view: self.view,
            time: Interval {
                smooth: true,
                ..time
            },
            from,
            dist: [d, dest],
            aim: [self.horizon.longitude, self.horizon.latitude],
            last: None,
            follow: !look || time.smooth,
        });
        Ok(())
    }
}

impl App {
    pub(crate) fn script_marker(&self, ui: &eframe::egui::Ui) {
        use eframe::egui::{self, Align2, Color32, Stroke};
        let Some(track) = self
            .motion
            .target
            .as_ref()
            .filter(|t| t.view == View::Solar && !matches!(t.object, Object::Body(_)))
        else {
            return;
        };
        let Some((p, _)) = track
            .marker
            .and_then(|p| self.solar.camera.project(p, self.canvas_rect))
            .filter(|(p, _)| self.canvas_rect.contains(*p))
        else {
            return;
        };
        ui.painter()
            .circle_stroke(p, 7., Stroke::new(1.5_f32, Color32::GOLD));
        ui.painter().text(
            p + egui::vec2(10., 7.),
            Align2::LEFT_TOP,
            &track.name,
            egui::FontId::proportional(12.),
            Color32::GOLD,
        );
    }
}
