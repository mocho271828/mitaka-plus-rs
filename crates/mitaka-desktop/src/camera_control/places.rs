//! Plus の Places 原本の識別子を使い、地平／地表では観測地点、宇宙では自転する地表点を追従する。
use super::{orientation, Interval};
use crate::{
    catalog::SkyData,
    projection::{Camera, View},
    solar::{Solar, RADII_KM},
    App,
};
use glam::DVec3;
use std::{io, time::Instant};

#[derive(Clone, Copy)]
pub(super) struct Place {
    pub body: usize,
    pub lat: f64,
    pub lon: f64,
}
impl Place {
    pub fn find(name: &str, sky: &SkyData) -> io::Result<Option<Self>> {
        let Some((prefix, body)) = [
            ("Planet.Earth.", 3),
            ("Planet.Mars.", 4),
            ("Satellite.Moon.", 10),
        ]
        .into_iter()
        .find(|(p, _)| name.starts_with(p)) else {
            return Ok(None);
        };
        let tail = &name[prefix.len()..];
        if tail.is_empty() || tail.contains('.') {
            return Err(io::Error::other("Places 識別子が不正です"));
        }
        let place = sky
            .layers
            .places
            .places
            .iter()
            .find(|p| p.id == name.as_bytes())
            .ok_or_else(|| io::Error::other("Places 原本に観測地点がありません"))?;
        Ok(Some(Self {
            body,
            lat: place.latitude,
            lon: place.longitude_signed(),
        }))
    }
    pub fn position(self, solar: &Solar, jd: f64) -> io::Result<[f64; 3]> {
        let center = DVec3::from_array(
            solar
                .body_point(self.body)
                .ok_or_else(|| io::Error::other("天体の位置がありません"))?,
        );
        let b = orientation::body_site(self.body, self.lat, self.lon, jd)?;
        Ok((center + b.y_axis * ((RADII_KM[self.body] + 0.002) / 149_597_870.7)).to_array())
    }
    pub fn orbit_orientation(self, jd: f64) -> io::Result<glam::DQuat> {
        Ok(orientation::horizon(
            Camera {
                longitude: 0.,
                latitude: -90.,
                ..Default::default()
            },
            orientation::body_site(self.body, self.lat, self.lon, jd)?,
        ))
    }
}
fn geographic(from: [f64; 2], to: [f64; 2], t: f64) -> [f64; 2] {
    if t <= 0. {
        return from;
    }
    if t >= 1. {
        return to;
    }
    let direction = |[lat, lon]: [f64; 2]| {
        let (s, c) = lat.to_radians().sin_cos();
        let (a, b) = lon.to_radians().sin_cos();
        DVec3::new(c * b, c * a, s)
    };
    let start = direction(from);
    let end = direction(to);
    let cos = start.dot(end).clamp(-1., 1.);
    let mut tangent = end - start * cos;
    if tangent.length() < 1e-12 {
        let axis = if start.z.abs() < 0.9 {
            DVec3::Z
        } else {
            DVec3::X
        };
        tangent = axis - start * start.dot(axis);
    }
    let (s, c) = (cos.acos() * t).sin_cos();
    let p = start * c + tangent.normalize() * s;
    [
        p.z.atan2(p.x.hypot(p.y)).to_degrees(),
        p.y.atan2(p.x).to_degrees(),
    ]
}

pub(super) struct Move {
    from: [f64; 2],
    to: Place,
    name: String,
    view: View,
    time: Interval,
}
impl App {
    pub(super) fn local_place(
        &mut self,
        name: &str,
        p: Place,
        time: Interval,
    ) -> Result<bool, String> {
        let body = match self.view {
            View::Horizon => 3,
            View::Surface => self.surface.as_ref().ok_or("地形データがありません")?.body,
            View::Solar => match self.motion.site.filter(|s| s.height <= 0.002000001) {
                Some(s) => s.body,
                None => return Ok(false),
            },
            _ => return Ok(false),
        };
        if body != p.body {
            return Err("同じ天体上の観測地点を選択してください".into());
        }
        let from = match self.view {
            View::Surface => self.surface.as_ref().map(|s| [s.lat, s.lon]),
            View::Solar => self.motion.site.map(|s| [s.lat, s.lon]),
            _ => Some([self.latitude, self.longitude]),
        }
        .ok_or("観測地点がありません")?;
        self.solar.selected = body;
        self.motion.target = None;
        self.motion.place = Some(Move {
            from,
            to: p,
            name: name.into(),
            view: self.view,
            time: Interval {
                smooth: true,
                ..time
            },
        });
        self.motion.place_name = Some(name.into());
        self.tick_place(time.start).map_err(|e| e.to_string())?;
        Ok(true)
    }
    pub(super) fn tick_place(&mut self, now: Instant) -> io::Result<()> {
        let Some(m) = self.motion.place.take() else {
            return Ok(());
        };
        if self.view != m.view {
            return Ok(());
        }
        let (t, done) = m.time.value(now);
        let [lat, lon] = geographic(m.from, [m.to.lat, m.to.lon], t);
        if m.view == View::Surface {
            self.surface
                .as_mut()
                .ok_or_else(|| io::Error::other("地形データがありません"))?
                .relocate(lat, lon)?;
        } else if let Some(s) = self.motion.site.as_mut().filter(|_| m.view == View::Solar) {
            s.lat = lat;
            s.lon = lon;
            s.last = None;
        }
        if m.to.body == 3 {
            self.latitude = lat;
            self.longitude = lon;
            self.options.layers.observer = if done {
                m.name.as_bytes().to_vec()
            } else {
                vec![]
            };
        }
        if !done {
            self.motion.place = Some(m);
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn place_move_crosses_seam_and_uses_polar_route_between_antipodes() {
        let [lat, lon] = geographic([45., 170.], [45., -170.], 0.5);
        assert!(lat > 45. && (lon.abs() - 180.).abs() < 1e-10);
        assert!((geographic([0., 0.], [0., 180.], 0.5)[0] - 90.).abs() < 1e-10);
        assert_eq!(geographic([90., 0.], [-90., 27.], 1.), [-90., 27.]);
    }
    #[test]
    #[ignore = "Plusの登録地点はMITAKA_DATA_ROOTから読み込む"]
    fn registered_places_on_three_bodies_resolve_to_surface_points() {
        let root = std::path::PathBuf::from(std::env::var_os("MITAKA_DATA_ROOT").unwrap());
        let mut sky = SkyData::open(&root).unwrap();
        assert!(Place::find("Planet.Earth.Tatesho", &sky).unwrap().is_some());
        assert!(Place::find("Planet.Earth.NAOJ", &sky).is_err());
        // 原版はユーザー定義ファイルがあると既定定義を使わないため、別々に検証する。
        sky.layers.places = mitaka_core::place_catalog::parse(
            &std::fs::read(root.join("defaultplaces_jp.dat")).unwrap(),
            Default::default(),
        )
        .unwrap();
        let mut solar = Solar::default();
        let jd = 2451545.;
        solar.prepare(&sky.ephemeris, jd).unwrap();
        for (name, body) in [
            ("Planet.Earth.NAOJ", 3),
            ("Planet.Mars.OlympusMons", 4),
            ("Satellite.Moon.NorthPole", 10),
        ] {
            let p = Place::find(name, &sky).unwrap().unwrap();
            assert_eq!(p.body, body);
            let delta = DVec3::from_array(p.position(&solar, jd).unwrap())
                - DVec3::from_array(solar.body_point(body).unwrap());
            assert!((delta.length() * 149_597_870.7 - RADII_KM[body] - 0.002).abs() < 1e-6);
        }
        assert!(Place::find("Planet.Mars.UnknownPlace", &sky).is_err());
    }
}
