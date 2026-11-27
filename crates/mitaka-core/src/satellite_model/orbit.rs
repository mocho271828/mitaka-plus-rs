//! Mitaka_plus f9fe624 satellite_orbit_geometry.cpp の同一日時・同一親の瞬間楕円。
use super::{KM_PER_AU, invalid};
use crate::{
    astronomy::Vector3,
    satellite::{SatelliteElements, TimedRotation},
};
use std::{
    io,
    sync::{Arc, OnceLock},
};

const SEGMENTS: usize = 256;
static UNIT_CIRCLE: OnceLock<[(f64, f64); SEGMENTS]> = OnceLock::new();

#[derive(Debug)]
pub(super) struct Ellipse {
    origin: Vector3,
    relative_km: Vector3,
    rotation: TimedRotation,
    eccentricity: f64,
    axis_km: f64,
    pub(super) factor: f64,
    points: OnceLock<Arc<[Vector3]>>,
}

impl Ellipse {
    pub(super) fn new(
        origin: Vector3,
        relative_km: Vector3,
        rotation: TimedRotation,
        elements: SatelliteElements,
    ) -> Self {
        Self {
            origin,
            relative_km,
            rotation,
            eccentricity: elements.eccentricity,
            axis_km: elements.semi_major_axis_km,
            factor: 1.,
            points: OnceLock::new(),
        }
    }

    pub(super) fn path(&self, jd: f64, marker: Vector3, cache: bool) -> io::Result<Arc<[Vector3]>> {
        let scale = self.factor / KM_PER_AU;
        let delta: Vector3 =
            std::array::from_fn(|i| self.origin[i] + self.relative_km[i] * scale - marker[i]);
        let error = delta[0].hypot(delta[1]).hypot(delta[2]);
        if jd != self.rotation.jd || !error.is_finite() || error >= 1e-10 {
            return Err(invalid(
                "satellite position and orbital plane do not share an epoch and focus",
            ));
        }
        if let Some(path) = self.points.get() {
            return Ok(Arc::clone(path));
        }
        let major = self.axis_km * scale;
        let minor = major * (1. - self.eccentricity * self.eccentricity).sqrt();
        let [axis_x, axis_y, _] = self.rotation.axes;
        let mut points = Vec::with_capacity(SEGMENTS + 1);
        let circle = UNIT_CIRCLE.get_or_init(|| {
            std::array::from_fn(|i| (std::f64::consts::TAU * i as f64 / SEGMENTS as f64).sin_cos())
        });
        for &(sin, cos) in circle {
            let x = major * (cos - self.eccentricity);
            let y = minor * sin;
            let point: Vector3 =
                std::array::from_fn(|i| self.origin[i] + x * axis_x[i] + y * axis_y[i]);
            if !point.iter().all(|v| v.is_finite()) {
                return Err(invalid("satellite orbit geometry overflow"));
            }
            points.push(point);
        }
        points.push(points[0]);
        let path: Arc<[Vector3]> = points.into();
        Ok(if cache {
            Arc::clone(self.points.get_or_init(|| path))
        } else {
            path
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rotated_ellipse_closes_and_checks_same_date_and_origin() {
        let jd = 2451545.;
        let origin = [1., 2., 3.];
        let a = 100000.;
        let e = 0.5;
        let mut orbit = Ellipse {
            origin,
            relative_km: [0., a * (1. - e), 0.],
            rotation: TimedRotation {
                jd,
                axes: [[0., 1., 0.], [-1., 0., 0.], [0., 0., 1.]],
            },
            eccentricity: e,
            axis_km: a,
            factor: 1.,
            points: OnceLock::new(),
        };
        for factor in [1., 1. - super::super::PLUTO_FRACTION] {
            orbit.factor = factor;
            let scale = factor / KM_PER_AU;
            let position = [1., 2. + 50000. * scale, 3.];
            let path = orbit.path(jd, position, false).unwrap();
            assert_eq!(path.len(), 257);
            assert_eq!(path.first(), path.last());
            for (p, target) in [
                (path[0], position),
                (
                    path[64],
                    [
                        1. - 100000. * 0.75_f64.sqrt() * scale,
                        2. - 50000. * scale,
                        3.,
                    ],
                ),
                (path[128], [1., 2. - 150000. * scale, 3.]),
            ] {
                assert!(p.iter().zip(target).all(|(a, b)| (a - b).abs() < 1e-14));
            }
            assert!(orbit.path(jd + 1., position, false).is_err());
            assert!(orbit.path(jd, origin, false).is_err());
        }
    }
}
