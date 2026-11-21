//! surface_navigation.cpp の自転する基準球、地上2 mの離着陸と対数高度補間。
use super::{orientation, session::Site};
use crate::{
    projection::{Camera, View},
    solar::{SolarCamera, RADII_KM},
    App,
};
use glam::DVec3;
use std::{io, time::Instant};
const AU: f64 = 149_597_870.7;
const GROUND: f64 = 0.002;

#[derive(Clone, Copy)]
pub(super) struct Flight {
    from: Site,
    to: Site,
    started: Instant,
    previous: Instant,
    seconds: f64,
    ascend: bool,
}
impl Flight {
    fn at(self, t: f64) -> Site {
        if t <= 0. {
            return self.from;
        }
        if t >= 1. {
            return self.to;
        }
        let f = t * t * (3. - 2. * t);
        let mut site = self.from;
        site.height = (self.from.height.ln() * (1. - f) + self.to.height.ln() * f).exp();
        site.elevation = self.from.elevation * (1. - f) + self.to.elevation * f;
        site.roll = (self.from.roll + super::signed(self.to.roll - self.from.roll) * f) % 360.;
        site.last = None;
        site
    }
}
impl Site {
    pub(super) fn on_sphere(body: usize, lat: f64, lon: f64) -> Self {
        Self {
            body,
            lat,
            lon,
            height: GROUND,
            heading: 0.,
            elevation: 0.,
            roll: 0.,
            terrain: 0.,
            last: None,
        }
    }
    pub(crate) fn from_camera(
        body: usize,
        center: DVec3,
        jd: f64,
        c: SolarCamera,
        force: bool,
    ) -> io::Result<Self> {
        let q = orientation::solar(c);
        let delta = DVec3::from_array(c.target) + q * DVec3::Z * c.distance - center;
        let distance = delta.length();
        let radius = RADII_KM[body];
        let mut height = distance * AU - radius;
        if !force && !(GROUND - 1e-6..=8. * radius + 1e-6).contains(&height) {
            return Err(io::Error::other("選択した惑星・月の近くへ移動してください"));
        }
        height = if force { GROUND } else { height.max(GROUND) };
        let prime = orientation::body_site(body, 0., 0., jd)?;
        let normal = if distance > 1e-15 {
            delta / distance
        } else {
            prime.y_axis
        };
        let x = normal.dot(prime.y_axis);
        let y = normal.dot(prime.x_axis);
        let lat = normal.dot(-prime.z_axis).atan2(x.hypot(y)).to_degrees();
        let lon = y.atan2(x).to_degrees();
        let local = orientation::body_site(body, lat, lon, jd)?;
        let forward = -(q * DVec3::Z);
        let east = forward.dot(local.x_axis);
        let north = forward.dot(-local.z_axis);
        let mut h = Camera::default();
        if north.hypot(east) <= 1e-12 {
            let right = q * DVec3::X;
            h.longitude = right
                .dot(local.z_axis)
                .atan2(right.dot(local.x_axis))
                .to_degrees()
                .rem_euclid(360.);
        }
        orientation::apply_horizon(&mut h, local, q);
        Ok(Self {
            height,
            heading: h.longitude,
            elevation: h.latitude,
            roll: if force { 0. } else { h.roll },
            ..Self::on_sphere(body, lat, lon)
        })
    }
}
impl App {
    pub(crate) fn nonterrain_mode(
        &mut self,
        ascend: bool,
        seconds: f64,
        force: bool,
    ) -> io::Result<()> {
        let body = self.solar.selected;
        if !(1..=10).contains(&body) {
            return Err(io::Error::other("離着陸には惑星または月を選択してください"));
        }
        if self.motion.physical_flight.is_some() && !force {
            return Ok(());
        }
        let was_ground = self
            .motion
            .site
            .is_some_and(|s| s.body == body && s.height <= GROUND + 1e-9);
        if self.motion.physical_flight.is_none() && was_ground != ascend {
            return Ok(());
        }
        let data = self
            .data
            .as_ref()
            .ok_or_else(|| io::Error::other("太陽系データがありません"))?;
        self.solar.prepare(&data.ephemeris, self.jd)?;
        let center = DVec3::from_array(
            self.solar
                .body_point(body)
                .ok_or_else(|| io::Error::other("選択天体の位置がありません"))?,
        );
        let from = if let Some(s) = self.motion.site.filter(|s| s.body == body) {
            s
        } else {
            Site::from_camera(body, center, self.jd, self.solar.camera, force)?
        };
        self.solar.stop();
        self.motion.target = None;
        self.motion.place = None;
        self.motion.place_name = None;
        self.motion.turn = None;
        self.motion.zoom = None;
        self.view = View::Solar;
        self.solar.show_controls = false;
        if ascend && force {
            self.motion.site = None;
            self.motion.physical_flight = None;
            self.motion.free_physical = true;
            return Ok(());
        }
        let mut to = from;
        to.height = if ascend {
            (8. * RADII_KM[body] - from.terrain).max(from.height)
        } else {
            GROUND
        };
        if ascend {
            to.elevation = -80.;
        }
        to.last = None;
        let now = Instant::now();
        self.motion.free_physical = false;
        self.motion.physical_flight = Some(Flight {
            from,
            to,
            started: now,
            previous: now,
            seconds,
            ascend,
        });
        self.tick_reference_flight(now)?;
        Ok(())
    }
    pub(super) fn tick_reference_flight(&mut self, now: Instant) -> io::Result<()> {
        let Some(mut f) = self.motion.physical_flight.take() else {
            return Ok(());
        };
        if self.view != View::Solar || self.solar.selected != f.from.body {
            return Ok(());
        }
        let t = if f.seconds == 0. {
            1.
        } else {
            (now.saturating_duration_since(f.started).as_secs_f64() / f.seconds).min(1.)
        };
        if now.saturating_duration_since(f.previous).as_secs_f64() > 1. {
            self.motion.site = None;
            self.motion.free_physical = true;
            return Ok(());
        }
        let mut s = f.at(t);
        s.last = None;
        s.advance(self)?;
        self.motion.site = Some(s);
        if t < 1. {
            f.previous = now;
            self.motion.physical_flight = Some(f);
        } else if f.ascend {
            self.motion.site = None;
            self.motion.free_physical = true;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::solar::Solar;
    use std::{
        fs::File,
        process::{Command, Stdio},
    };
    #[test]
    fn flight_interpolates_height_in_kilometers_and_keeps_true_endpoints() {
        let from = Site {
            height: 100.,
            elevation: -80.,
            ..Site::on_sphere(5, 0., 0.)
        };
        let to = Site {
            height: 0.002,
            elevation: 0.,
            ..from
        };
        let now = Instant::now();
        let f = Flight {
            from,
            to,
            started: now,
            previous: now,
            seconds: 3.,
            ascend: false,
        };
        assert_eq!(f.at(0.).height, 100.);
        assert_eq!(f.at(1.).height, 0.002);
        assert!((f.at(0.5).height - (100_f64 * 0.002).sqrt()).abs() < 1e-12);
        assert_eq!(f.at(0.5).elevation, -40.);
    }
    #[test]
    #[ignore = "MITAKA_DATA_ROOT と MITAKA_REFERENCE_SESSION を使い全10天体を原版C++と比較"]
    fn shipped_poses_and_v9_match_every_native_landing_body() {
        let root = std::path::PathBuf::from(std::env::var_os("MITAKA_DATA_ROOT").unwrap());
        let native = std::env::var_os("MITAKA_REFERENCE_SESSION").unwrap();
        let dir = tempfile::tempdir().unwrap();
        let data = crate::catalog::SkyData::open(&root).unwrap();
        let mut solar = Solar::default();
        let jd = 2451545.;
        solar.prepare(&data.ephemeris, jd).unwrap();
        for line in include_str!("native_surface_test.txt").lines() {
            let row: Vec<f64> = line
                .split_ascii_whitespace()
                .map(|n| n.parse::<f64>().unwrap())
                .collect();
            let body = row[0] as usize;
            let mut c = SolarCamera::default();
            let site = Site {
                heading: 47.,
                elevation: -16.,
                roll: 13.,
                ..Site::on_sphere(body, 21.25, -106.5)
            };
            let center = DVec3::from_array(solar.body_point(body).unwrap());
            site.pose(center, jd, &mut c).unwrap();
            let values = std::iter::once(c.distance)
                .chain(c.target)
                .chain(c.basis().into_iter().flatten());
            for (a, b) in values.zip(&row[1..]) {
                assert!((a - b).abs() < 4e-11, "body {body}, {a} vs {b}");
            }
            let restored = Site::from_camera(body, center, jd, c, false).unwrap();
            for (a, b) in [
                (site.lat, restored.lat),
                (site.lon, restored.lon),
                (site.heading, restored.heading),
                (site.roll, restored.roll),
            ] {
                assert!((a - b).abs() < 0.00001, "{body}: {a} vs {b}");
            }
            let mut session = crate::session::Session::default();
            let v = &mut session.0;
            v["mode"] = 1.into();
            v["selected_body"] = body.into();
            v["surface_site"] = site.value();
            v["solar_camera"]["physical_space"] = false.into();
            v["solar_camera"]["target"] = serde_json::json!(c.target);
            v["solar_camera"]["distance_au"] = c.distance.into();
            v["solar_camera"]["frame_is_orbit"] = false.into();
            let [right, up, outward] = c.basis();
            v["solar_camera"]["frame"] =
                serde_json::json!({"right":right,"up":up,"outward":outward});
            let path = dir.path().join("site.json");
            session.write(&path).unwrap();
            let checked = Command::new(&native)
                .arg("decode")
                .stdin(Stdio::from(File::open(&path).unwrap()))
                .output()
                .unwrap();
            assert!(
                checked.status.success(),
                "legacy decoder rejected body {body}"
            );
            let round = crate::session::Session::decode(&checked.stdout).unwrap();
            for (key, n) in session.0["surface_site"].as_object().unwrap() {
                assert_eq!(round.0["surface_site"][key].as_f64(), n.as_f64(), "{key}");
            }
        }
    }
}
