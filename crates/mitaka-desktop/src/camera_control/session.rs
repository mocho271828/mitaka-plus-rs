use super::orientation;
use crate::{
    projection::{Camera, View},
    App,
};
use glam::DVec3;
use serde_json::{json, Value};
use std::io;

#[derive(Clone, Copy, Debug)]
pub(crate) struct Site {
    pub body: usize,
    pub lat: f64,
    pub lon: f64,
    pub height: f64,
    pub heading: f64,
    pub elevation: f64,
    pub roll: f64,
    pub terrain: f64,
    pub(super) last: Option<f64>,
}
impl Site {
    pub(crate) fn decode(v: &Value) -> io::Result<Option<Self>> {
        let Some(s) = v.get("surface_site") else {
            return Ok(None);
        };
        let n = |k: &str| {
            s[k].as_f64()
                .ok_or_else(|| io::Error::other(format!("surface_site.{k}")))
        };
        Ok(Some(Self {
            body: n("body_index")? as usize,
            lat: n("latitude_deg")?,
            lon: n("longitude_deg")?,
            height: n("height_km")?,
            heading: n("heading_deg")?,
            elevation: n("elevation_deg")?,
            roll: n("roll_deg")?,
            terrain: n("terrain_offset_km")?,
            last: Some(v["julian_date"].as_f64().unwrap()),
        }))
    }
    pub(super) fn from_surface(s: &crate::surface::Surface, horizon: Camera) -> Self {
        Self {
            body: s.body,
            lat: s.lat,
            lon: s.lon,
            height: 0.002,
            heading: horizon.longitude,
            elevation: horizon.latitude,
            roll: horizon.roll,
            terrain: s.landing_height_m(crate::solar::RADII_KM[s.body]) / 1000.,
            last: None,
        }
    }
    pub(crate) fn pose(
        &self,
        center: DVec3,
        jd: f64,
        c: &mut crate::solar::SolarCamera,
    ) -> io::Result<()> {
        let local = orientation::body_site(self.body, self.lat, self.lon, jd)?;
        let q = orientation::horizon(
            Camera {
                longitude: self.heading,
                latitude: self.elevation,
                roll: self.roll,
                zoom: 1.,
            },
            local,
        );
        let dist = crate::solar::Solar::body_distance(self.body) / 6.
            + (self.height + self.terrain) / 149_597_870.7;
        if dist <= 0. || !dist.is_finite() {
            return Err(io::Error::other("地表の半径が不正です"));
        }
        c.target = (center + dist * (local.y_axis - q * DVec3::Z)).to_array();
        c.distance = dist;
        orientation::apply_solar(c, q);
        c.frame_is_orbit = false;
        Ok(())
    }
    pub(super) fn value(self) -> Value {
        json!({"body_index":self.body,"latitude_deg":self.lat,"longitude_deg":self.lon,"height_km":self.height,"heading_deg":self.heading,"elevation_deg":self.elevation,"roll_deg":self.roll,"terrain_offset_km":self.terrain})
    }
    pub(super) fn advance(&mut self, app: &mut App) -> io::Result<()> {
        if self.last == Some(app.jd) || app.view != View::Solar {
            return Ok(());
        }
        let sky = app
            .data
            .as_ref()
            .ok_or_else(|| io::Error::other("天体データがありません"))?;
        app.solar.prepare(&sky.ephemeris, app.jd)?;
        let center = DVec3::from_array(
            app.solar
                .body_point(self.body)
                .ok_or_else(|| io::Error::other("天体位置を取得できません"))?,
        );
        self.pose(center, app.jd, &mut app.solar.camera)?;
        self.last = Some(app.jd);
        Ok(())
    }
}
impl App {
    pub(crate) fn session_camera(&self, v: &mut Value) -> io::Result<()> {
        let mut c = self.solar.camera;
        let site = if self.view == View::Surface {
            let ground = self
                .surface
                .as_ref()
                .ok_or_else(|| io::Error::other("地形がありません"))?;
            let site = Site::from_surface(ground, self.horizon);
            let ephemeris = &self
                .data
                .as_ref()
                .ok_or_else(|| io::Error::other("天体データがありません"))?
                .ephemeris;
            let center = crate::solar::planet(ephemeris, site.body - 1, self.jd)?;
            site.pose(center.into(), self.jd, &mut c)?;
            Some(site)
        } else {
            self.motion
                .site
                .filter(|s| self.view == View::Solar && (s.body == 3 || s.height <= 0.002000001))
        };

        let frame = c
            .frame
            .map(|[right, up, outward]| json!({"right":right,"up":up,"outward":outward}));
        let mut solar = json!({"target":c.target,"distance_au":c.distance,"yaw_deg":c.yaw,"pitch_deg":c.pitch.clamp(-89.,89.),"roll_deg":c.roll,"fov_deg":c.fov,"frame":frame,"frame_is_orbit":c.frame_is_orbit});
        if let Some(s) = site {
            v["surface_site"] = s.value();
            solar["physical_space"] = json!(s.height > 0.002000001);
        } else {
            v.as_object_mut().unwrap().remove("surface_site");
            if self.motion.free_physical || self.motion.physical_flight.is_some() {
                solar["physical_space"] = json!(true);
            }
        }
        if let Some(target) = c.precise() {
            solar["precise_target"] =
                json!({"anchor_pc":target.anchor_pc,"offset_au":target.offset_au});
        }
        v["solar_camera"] = solar;
        Ok(())
    }
    pub(crate) fn camera_extras_after_restore(&mut self, c: &Value, site: Option<Site>) {
        self.motion = Default::default();
        self.motion.site = site;
        self.motion.free_physical = site.is_none() && c["physical_space"].as_bool() == Some(true);
        self.solar.camera.frame = if c["frame"].is_null() {
            None
        } else {
            Some(
                ["right", "up", "outward"]
                    .map(|key| std::array::from_fn(|i| c["frame"][key][i].as_f64().unwrap())),
            )
        };
        self.solar.camera.fov = c["fov_deg"].as_f64().unwrap();
        self.solar.camera.frame_is_orbit = c["frame_is_orbit"].as_bool().unwrap_or(false);
    }
}
pub(crate) fn saved_site(v: &Value) -> io::Result<Option<Site>> {
    Site::decode(v)
}

impl App {
    pub(crate) fn check_camera_session(&mut self) -> io::Result<()> {
        let dir = tempfile::tempdir()?;
        let original = dir.path().join("original.json");
        let edited = dir.path().join("ground.json");
        let saved = dir.path().join("restored.json");
        self.save_session(&original)?;
        let mut state = crate::session::Session::read(&original)?;
        let v = &mut state.0;
        v["mode"] = json!(1);
        v["selected_body"] = json!(3);
        v["surface_site"] = json!({"body_index":3,"latitude_deg":20.0,"longitude_deg":140.0,"height_km":0.002,"heading_deg":30.0,"elevation_deg":-15.0,"roll_deg":7.,"terrain_offset_km":0.09});
        v["solar_camera"]["fov_deg"] = json!(33.);
        v["solar_camera"]["physical_space"] = json!(false);
        v["solar_camera"]["frame_is_orbit"] = json!(false);
        v["solar_camera"]["frame"] =
            json!({"right":[0.,1.,0.],"up":[0.,0.,1.],"outward":[1.,0.,0.]});
        v["horizon_viewport"]["roll_deg"] = json!(8.);
        state.write(&edited)?;
        self.restore_session(&edited)?;
        self.save_session(&saved)?;
        let checked = crate::session::Session::read(&saved)?;
        for key in ["surface_site", "solar_camera", "horizon_viewport"] {
            if checked.0[key] != state.0[key] {
                return Err(io::Error::other(format!("v9の復元で {key} が変化しました")));
            }
        }
        self.restore_session(&original)
    }
}
