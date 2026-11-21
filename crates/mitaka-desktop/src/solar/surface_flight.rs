//! Mitaka_plus f9fe624 surface_navigation.cpp と AppController.mm の地球地平図遷移。
use super::{angle_lerp, SolarCamera, RADII_KM};
use crate::{camera_control::session::Site, projection::Camera};
use std::time::Instant;
pub(super) struct SurfaceFlight {
    pub started: Instant,
    pub duration: f64,
    pub progress: f64,
    pub ascend: bool,
    pub from: Site,
    pub to: Site,
    pub horizon: bool,
}
impl SurfaceFlight {
    pub fn new(ground: Site, source: Option<Site>, ascend: bool) -> Self {
        let earth = ground.body == 3;
        let radius = RADII_KM[ground.body];
        let mut from = if ascend {
            ground
        } else {
            source.unwrap_or(Site {
                height: 8. * radius - ground.terrain,
                elevation: -80.,
                ..ground
            })
        };
        let same = from.at_location(ground.body, ground.lat, ground.lon);
        if same {
            from.lat = ground.lat;
            from.lon = ground.lon;
        }
        let mut to = if same {
            ground
        } else {
            Site {
                height: 0.002,
                ..from
            }
        };
        if ascend {
            to.height = if earth {
                radius * 7.5 * ((400. + 217.3) / 2000.)
            } else {
                radius * 8.
            } - ground.terrain;
            to.height = to.height.max(from.height);
            to.elevation = if earth { -90. } else { -80. };
        }
        Self {
            started: Instant::now(),
            duration: 3.,
            progress: 0.,
            ascend,
            from,
            to,
            horizon: earth && same,
        }
    }
    pub fn update_terrain(&mut self, site: Site) {
        self.to.update_terrain(site.terrain);
        if self.horizon {
            let remaining = 0.5 - 0.5 * ((1. - self.progress) * std::f64::consts::PI).cos();
            if remaining > 0. {
                self.from.height = self.to.height + (site.height - self.to.height) / remaining;
            }
            self.from.terrain = site.terrain;
        } else {
            // 対数高度の残区間を現在姿勢から開始し、元の到着時刻を保つ。
            self.started += std::time::Duration::from_secs_f64(self.duration * self.progress);
            self.duration *= 1. - self.progress;
            self.progress = 0.;
            self.from = site;
        }
    }
    pub fn at(&self, t: f64) -> Site {
        if t <= 0. {
            return self.from;
        }
        if t >= 1. {
            return self.to;
        }
        let smooth = t * t * (3. - 2. * t);
        let (height, look) = if self.horizon {
            let w = 0.5 - 0.5 * (t * std::f64::consts::PI).cos();
            let h = self.from.height * (1. - w) + self.to.height * w;
            let start = look_weight(self.from.height + self.from.terrain);
            let end = look_weight(self.to.height + self.from.terrain);
            let f = if start == end {
                smooth
            } else {
                ((look_weight(h + self.from.terrain) - start) / (end - start)).clamp(0., 1.)
            };
            (h, f)
        } else {
            (
                (self.from.height.ln() * (1. - smooth) + self.to.height.ln() * smooth).exp(),
                smooth,
            )
        };
        Site {
            height,
            heading: angle_lerp(self.from.heading, self.to.heading, smooth).rem_euclid(360.),
            roll: angle_lerp(self.from.roll, self.to.roll, smooth),
            elevation: self.from.elevation * (1. - look) + self.to.elevation * look,
            ..self.from
        }
    }
}
fn look_weight(h: f64) -> f64 {
    let r = 1. + h / RADII_KM[3];
    if r <= 1.002 {
        return 1.;
    }
    if r >= 2. {
        return 0.;
    }
    let outer = (1.002_f64 / 2.).asin();
    let p = ((1.002 / r).asin() - outer) / (std::f64::consts::FRAC_PI_2 - outer);
    if p < 0.5 {
        2. * p * p
    } else {
        1. - 2. * (1. - p).powi(2)
    }
}
#[derive(Clone, Copy)]
pub(crate) struct Overlay {
    pub camera: Camera,
    pub blend: f64,
    pub fov: f64,
    pub rect: eframe::egui::Rect,
}
impl Overlay {
    pub(super) fn new(
        s: Site,
        c: SolarCamera,
        ground: Camera,
        rect: eframe::egui::Rect,
    ) -> Option<Self> {
        let p = ((s.height - 0.002) / (0.01 * RADII_KM[3])).clamp(0., 1.);
        let blend = p * p * (3. - 2. * p);
        (blend < 1.).then_some(Self {
            camera: Camera {
                longitude: s.heading,
                latitude: s.elevation,
                roll: s.roll,
                zoom: ground.zoom,
            },
            blend,
            fov: c.fov,
            rect,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn earth_negative_terrain_takeoff_and_return_follow_native_displayed_site() {
        let camera = Camera {
            longitude: 214.,
            latitude: 17.,
            roll: -21.,
            zoom: 1.5,
        };
        let ground = Site::ground(3, 35., 139., -700., camera);
        let flight = SurfaceFlight::new(ground, None, true);
        for (t, h, e) in [
            (0., 0.002, 17.),
            (0.01, 3.6448888991801, 17.),
            (0.03, 32.766433091352, 15.821098857904),
            (0.1, 361.33429090918, -3.3687291534855),
            (0.25, 2162.3282124913, -69.867080572039),
            (0.5, 7382.6454809625, -90.),
            (0.75, 12602.962749434, -90.),
            (1., 14765.288961925, -90.),
        ] {
            let s = flight.at(t);
            assert!((s.height - h).abs() < 1e-8 && (s.elevation - e).abs() < 1e-9);
            assert_eq!(s.heading, 214.);
            assert_eq!(s.roll, -21.);
        }
        let arrival = SurfaceFlight::new(ground, Some(flight.to), false);
        let end = arrival.at(1.);
        assert_eq!(
            [end.height, end.heading, end.elevation, end.roll],
            [0.002, 214., 17., -21.]
        );
        let other = Site::ground(3, 35., -41., -700., camera);
        assert!(!SurfaceFlight::new(other, Some(flight.to), false).horizon);
    }
}
#[cfg(test)]
mod changed_surface {
    use super::*;
    #[test]
    fn standing_follows_the_replacement_mesh_and_flying_preserves_datum_height() {
        let mut ground = Site::ground(4, 28., 35., -700., Camera::default());
        let mut high = Site {
            height: 10.,
            ..ground
        };
        ground.update_terrain(-14.);
        high.update_terrain(-14.);
        assert_eq!(ground.height, 0.002);
        assert!((high.height + high.terrain - 9.3).abs() < 1e-12);
        high.update_terrain(10.);
        assert_eq!(high.height, 0.002);
    }
    #[test]
    fn midflight_replacement_keeps_current_and_destination_datum_radii() {
        for body in [3, 4] {
            let start = Site::ground(body, 28., 35., -700., Camera::default());
            let mut flight = SurfaceFlight::new(start, None, true);
            flight.progress = 0.37;
            let mut site = flight.at(flight.progress);
            let h = site.height + site.terrain;
            let end = flight.to.height + flight.to.terrain;
            site.update_terrain(-14.);
            flight.update_terrain(site);
            let next = flight.at(flight.progress);
            assert!((next.terrain + next.height - h).abs() < 1e-9);
            assert!((flight.to.height + flight.to.terrain - end).abs() < 1e-9);
        }
    }
}
