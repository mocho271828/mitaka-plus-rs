//! interplanetary_flight.cpp (Mitaka_plus f9fe624) の天体間飛行。距離は AU、高度は km。
use super::{orientation, session::Site};
use crate::solar::{SolarCamera, RADII_KM};
use glam::{DQuat, DVec3};
use std::{io, time::Instant};

const ASCENT_END: f64 = 4.;
const CRUISE_END: f64 = 20.;
pub(super) const DURATION: f64 = 24.;
type Centers = [[f64; 3]; 11];

#[derive(Clone, Copy)]
struct Sphere {
    center: DVec3,
    radius: f64,
}
impl Sphere {
    fn crosses(self, a: DVec3, b: DVec3) -> bool {
        let delta = b - a;
        let squared = delta.length_squared();
        let t = if squared > 0. {
            ((self.center - a).dot(delta) / squared).clamp(0., 1.)
        } else {
            0.
        };
        a.lerp(b, t).distance_squared(self.center) < self.radius * self.radius
    }
}
fn obstacles(centers: &Centers) -> [Sphere; 11] {
    std::array::from_fn(|i| Sphere {
        center: centers[i].into(),
        radius: if i == 0 { 696000. } else { RADII_KM[i] } / 149_597_870.7,
    })
}
fn collision() -> io::Error {
    io::Error::other("経路が天体の内部を通るため飛行を中断しました")
}
fn eye(c: SolarCamera) -> DVec3 {
    DVec3::from_array(c.target) + DVec3::from_array(c.basis()[2]) * c.distance
}
fn upper(mut site: Site) -> Site {
    site.height = site.height.max(RADII_KM[site.body] * 8.);
    site.elevation = -80.;
    site
}
fn smooth(t: f64) -> f64 {
    let t = t.clamp(0., 1.);
    t * t * (3. - 2. * t)
}
fn fraction(t: f64, from_site: bool) -> f64 {
    let main = smooth((t - ASCENT_END) / (CRUISE_END - ASCENT_END));
    if from_site {
        main
    } else {
        0.1 * smooth(t / ASCENT_END) + 0.9 * main
    }
}
fn blend(a: Site, b: Site, t: f64) -> Site {
    if t >= 1. {
        return b;
    }
    Site {
        height: (a.height.ln() * (1. - t) + b.height.ln() * t)
            .exp()
            .clamp(0.002, 1e9),
        elevation: a.elevation + (b.elevation - a.elevation) * t,
        ..a
    }
}
fn pose(s: Site, centers: &Centers, jd: f64, mut c: SolarCamera) -> io::Result<SolarCamera> {
    s.pose(centers[s.body].into(), jd, &mut c)?;
    Ok(c)
}
#[derive(Clone, Copy)]
struct Waypoint {
    fraction: f64,
    offset: DVec3,
}
struct Route {
    points: Vec<Waypoint>,
    relative: DQuat,
}
impl Route {
    fn new(
        from: SolarCamera,
        to: SolarCamera,
        normal: DVec3,
        obstacles: &[Sphere],
    ) -> io::Result<Self> {
        let (a, b) = (eye(from), eye(to));
        if obstacles.iter().any(|s| s.crosses(a, a) || s.crosses(b, b)) {
            return Err(collision());
        }
        let mut points = vec![a, b];
        let mut i = 0;
        while i + 1 < points.len() {
            let (p, q) = (points[i], points[i + 1]);
            let Some(s) = obstacles.iter().find(|s| s.crosses(p, q)) else {
                i += 1;
                continue;
            };
            if points.len() >= 96 {
                return Err(collision());
            }
            let forward = (q - p).normalize();
            let mut side = normal.cross(forward);
            if side.length() < 1e-8 {
                side = (if forward.x.abs() < 0.8 {
                    DVec3::X
                } else {
                    DVec3::Y
                })
                .cross(forward);
            }
            let clearance = 2. * s.radius;
            let mut detour = s.center + side.normalize() * clearance;
            let outward = p - s.center;
            if i == 0 && outward.length() < clearance && s.crosses(p, detour) {
                detour = s.center + outward.normalize() * clearance;
            }
            points.insert(i + 1, detour);
        }
        let total: f64 = points.windows(2).map(|w| w[0].distance(w[1])).sum();
        let mut traveled = 0.;
        let points = points
            .iter()
            .enumerate()
            .map(|(i, &p)| {
                if i > 0 {
                    traveled += points[i - 1].distance(p);
                }
                let f = if total > 1e-15 {
                    traveled / total
                } else {
                    i as f64 / (points.len() - 1) as f64
                };
                Waypoint {
                    fraction: f,
                    offset: p - a.lerp(b, f),
                }
            })
            .collect();
        Ok(Self {
            points,
            relative: orientation::solar(to).conjugate() * orientation::solar(from),
        })
    }
    fn position(&self, a: DVec3, b: DVec3, f: f64) -> DVec3 {
        let i = self.points.partition_point(|v| v.fraction <= f);
        let lower = self.points[i.saturating_sub(1)];
        let upper = self.points[i.min(self.points.len() - 1)];
        let t = if upper.fraction > lower.fraction {
            (f - lower.fraction) / (upper.fraction - lower.fraction)
        } else {
            0.
        };
        a.lerp(b, f) + lower.offset.lerp(upper.offset, t)
    }
}

pub(super) struct Journey {
    source: Option<Site>,
    start: SolarCamera,
    pub destination: Site,
    normal: DVec3,
    route: Option<Route>,
    previous: Instant,
    pub elapsed: f64,
}
impl Journey {
    pub fn new(
        source: Option<Site>,
        start: SolarCamera,
        destination: Site,
        centers: &Centers,
        jd: f64,
        now: Instant,
    ) -> io::Result<Self> {
        let c = if let Some(s) = source {
            pose(s, centers, jd, start)?
        } else {
            start
        };
        let end = pose(upper(destination), centers, jd, start)?;
        let a = eye(c);
        let bodies = obstacles(centers);
        if bodies.iter().any(|s| s.crosses(a, a)) {
            return Err(collision());
        }
        let direction = (eye(end) - a).normalize_or_zero();
        let normal = direction
            .cross(if direction.z.abs() < 0.8 {
                DVec3::Z
            } else {
                DVec3::Y
            })
            .try_normalize()
            .unwrap_or(DVec3::Z);
        let route = if source.is_none() {
            Some(Route::new(c, end, normal, &bodies)?)
        } else {
            None
        };
        Ok(Self {
            source,
            start: c,
            destination,
            normal,
            route,
            previous: now,
            elapsed: 0.,
        })
    }
    fn endpoints(&self, c: &Centers, jd: f64) -> io::Result<(SolarCamera, SolarCamera)> {
        Ok((
            match self.source {
                Some(s) => pose(upper(s), c, jd, self.start)?,
                None => self.start,
            },
            pose(upper(self.destination), c, jd, self.start)?,
        ))
    }
    fn camera(
        &self,
        t: f64,
        c: &Centers,
        jd: f64,
        ends: (SolarCamera, SolarCamera),
    ) -> io::Result<SolarCamera> {
        if let Some(s) = self.source.filter(|_| t <= ASCENT_END) {
            return pose(
                blend(s, upper(s), smooth(t / ASCENT_END)),
                c,
                jd,
                self.start,
            );
        }
        if t >= CRUISE_END {
            return pose(
                blend(
                    upper(self.destination),
                    self.destination,
                    smooth((t - CRUISE_END) / (DURATION - CRUISE_END)),
                ),
                c,
                jd,
                self.start,
            );
        }
        let route = self
            .route
            .as_ref()
            .ok_or_else(|| io::Error::other("巡航経路がありません"))?;
        let (a, b) = ends;
        let f = fraction(t, self.source.is_some());
        let q = orientation::solar(b) * route.relative.slerp(DQuat::IDENTITY, f);
        let d = a.distance * (1. - f) + b.distance * f;
        let mut camera = self.start;
        orientation::apply_solar(&mut camera, q);
        camera.frame_is_orbit = false;
        camera.distance = d;
        camera.target = (route.position(eye(a), eye(b), f) - q * DVec3::Z * d).to_array();
        Ok(camera)
    }
    pub fn advance(&mut self, now: Instant, c: &Centers, jd: f64) -> io::Result<SolarCamera> {
        let dt = now
            .checked_duration_since(self.previous)
            .map(|t| t.as_secs_f64())
            .filter(|t| *t <= 1.)
            .ok_or_else(|| io::Error::other("画面更新が1秒以上途切れたため飛行を中断しました"))?;
        let next = (self.elapsed + dt).min(DURATION);
        let bodies = obstacles(c);
        let ends = self.endpoints(c, jd)?;
        if next >= ASCENT_END && self.route.is_none() {
            self.route = Some(Route::new(ends.0, ends.1, self.normal, &bodies)?);
        }
        let check = |a, b| {
            if bodies.iter().any(|s| s.crosses(a, b)) {
                Err(collision())
            } else {
                Ok(())
            }
        };
        if let Some(route) = self.route.as_ref().filter(|_| self.elapsed <= CRUISE_END) {
            let first = fraction(self.elapsed, self.source.is_some());
            let last = fraction(next, self.source.is_some());
            let (a, b) = (eye(ends.0), eye(ends.1));
            let mut prev = route.position(a, b, first);
            for w in route
                .points
                .iter()
                .filter(|w| first < w.fraction && w.fraction < last)
            {
                let p = route.position(a, b, w.fraction);
                check(prev, p)?;
                prev = p;
            }
            check(prev, route.position(a, b, last))?;
        }
        let mut camera = self.camera(self.elapsed, c, jd, ends)?;
        let steps = (dt / 0.01).ceil().max(1.) as u32;
        for i in 1..=steps {
            let t = self.elapsed + (next - self.elapsed) * f64::from(i) / f64::from(steps);
            let following = self.camera(t, c, jd, ends)?;
            check(eye(camera), eye(following))?;
            camera = following;
        }
        self.previous = now;
        self.elapsed = next;
        Ok(camera)
    }
}

impl Journey {
    pub fn start_camera(&self) -> SolarCamera {
        self.start
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    fn centers() -> Centers {
        let mut c: Centers = std::array::from_fn(|i| [i as f64, 0., 0.]);
        c[3] = [-1., 0., 0.];
        c[4] = [1., 0., 0.];
        c[1] = [0., 3., 0.];
        c[10] = [-0.9975, 0., 0.];
        c
    }
    #[test]
    fn detours_keep_every_segment_outside_the_sun() {
        let (mut a, mut b) = (SolarCamera::default(), SolarCamera::default());
        a.target = [-1., 0., 0.];
        b.target = [1., 0., 0.];
        a.distance = 0.;
        b.distance = 0.;
        let spheres = obstacles(&centers());
        let route = Route::new(a, b, DVec3::Y, &spheres[..1]).unwrap();
        assert!(route.points.len() > 2);
        for w in route.points.windows(2) {
            let point = |p: Waypoint| eye(a).lerp(eye(b), p.fraction) + p.offset;
            assert!(!spheres[0].crosses(point(w[0]), point(w[1])));
        }
    }
    #[test]
    fn same_eyes_and_orientations_as_native_interplanetary_flight() {
        let now = Instant::now();
        let c = centers();
        let jd = 2451545.;
        let source = Site::on_sphere(3, 35., 139.);
        let dest = Site {
            terrain: 1.25,
            ..Site::on_sphere(4, -17., 113.)
        };
        let mut flight =
            Journey::new(Some(source), SolarCamera::default(), dest, &c, jd, now).unwrap();
        let mut reference = include_str!("native_journey.txt").lines();
        for step in 0..=48 {
            let camera = flight
                .advance(now + Duration::from_millis(step * 500), &c, jd)
                .unwrap();
            if step % 4 != 0 {
                continue;
            }
            let line = reference.next().unwrap();
            let native = line.split_whitespace().map(|x| x.parse::<f64>().unwrap());
            let values = [flight.elapsed, camera.distance]
                .into_iter()
                .chain(camera.target)
                .chain(camera.basis().into_iter().flatten());
            for (rust, expected) in values.zip(native) {
                assert!(
                    (rust - expected).abs() < 1e-10,
                    "time {}: {rust} vs {expected}",
                    flight.elapsed
                );
            }
        }
        assert!(reference.next().is_none());
    }
    #[test]
    fn ten_destinations_and_resumed_flights_land_at_the_current_site_pose() {
        let c = centers();
        let now = Instant::now();
        let jd = 2451545.;
        let original = Site::on_sphere(3, 35., 139.);
        for body in 1..=10 {
            let site = Site {
                terrain: 1.25,
                ..Site::on_sphere(body, -17., 113.)
            };
            let mut f =
                Journey::new(Some(original), SolarCamera::default(), site, &c, jd, now).unwrap();
            let mut camera = f.start_camera();
            for i in 1..=192 {
                camera = f
                    .advance(
                        now + Duration::from_millis(i * 125),
                        &c,
                        jd + i as f64 / 864000.,
                    )
                    .unwrap();
            }
            let expected = pose(site, &c, jd + 192. / 864000., camera).unwrap();
            assert!(eye(camera).distance(eye(expected)) < 1e-11);
            assert!(orientation::solar(camera).angle_between(orientation::solar(expected)) < 1e-7);
            assert_eq!(f.elapsed, DURATION);
        }
        let site = Site::on_sphere(4, 3., 4.);
        let mut f = Journey::new(None, SolarCamera::default(), site, &c, jd, now).unwrap();
        let camera = f.advance(now + Duration::from_millis(500), &c, jd).unwrap();
        assert!(f.advance(now + Duration::from_secs(2), &c, jd).is_err());
        assert_eq!(f.elapsed, 0.5);
        let mut restart = Journey::new(None, camera, site, &c, jd, now).unwrap();
        assert!(eye(camera).distance(eye(restart.advance(now, &c, jd).unwrap())) < 1e-12);
        let mut blocked = c;
        blocked[5] = eye(camera).to_array();
        assert!(restart.advance(now, &blocked, jd).is_err());
    }
}
