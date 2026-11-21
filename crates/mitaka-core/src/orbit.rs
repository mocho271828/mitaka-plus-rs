/*
Derived from Mitaka 1.3.1: common/JulianDate.cpp, common/astro/cele_mech.cpp,
common/astro/astrometry.cpp, common/math/vector3.h,
Mitaka/mtk_orbital_element.cpp and Mitaka/mtk_planet_position.cpp.

Copyright (c) 2007   Tsunehiko Kato
Copyright (c) 2007-2012   Tsunehiko Kato
Copyright (c) 2007-2014   Tsunehiko Kato
Copyright (c) 2013   Tsunehiko Kato
Copyright (c) 2007, 2014   Tsunehiko Kato and 4D2U Project, National Astronomical Observatory of Japan
Copyright (c) 2007-2015   Tsunehiko Kato and 4D2U Project, National Astronomical Observatory of Japan

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in
all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
THE SOFTWARE.
*/
//! Keplerian states and the original nine mean-element models. The output is
//! heliocentric ecliptic; these models are distinct from the CF96 ephemeris.
use crate::astronomy::{Vector3, centered, rotate_x, rotate_z};
use std::{
    f64::consts::{PI, TAU},
    io,
};

#[derive(Clone, Copy, Debug)]
pub struct OrbitalElements {
    pub epoch_jd: f64,
    pub semi_major_axis_au: f64,
    pub eccentricity: f64,
    pub mean_anomaly_deg: f64,
    pub mean_motion_deg_per_day: f64,
    pub ascending_node_deg: f64,
    pub inclination_deg: f64,
    pub argument_of_periapsis_deg: f64,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OrbitalState {
    pub position_au: Vector3,
    pub velocity_au_per_day: Vector3,
}
fn invalid(reason: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, reason)
}
fn eccentric_anomaly(e: f64, mean: f64) -> io::Result<f64> {
    let elliptic = e < 1.;
    let m = if elliptic { centered(mean, TAU) } else { mean };
    if m == 0. {
        return Ok(0.);
    }
    let target = m.abs();
    if !elliptic {
        let linear = target / (e - 1.);
        if linear == 0. {
            return Err(invalid("hyperbolic anomaly underflow"));
        }
        if linear <= (f64::EPSILON * ((e - 1.) / e)).sqrt() {
            return Ok(linear.copysign(m));
        }
    }
    let mut lower = 0.;
    let mut upper = if elliptic {
        PI
    } else {
        (target / e).asinh() + 1.
    };
    for _ in 0..160 {
        let x = (lower + upper) / 2.;
        let residual = if elliptic {
            x - e * x.sin() - target
        } else {
            e * x.sinh() - x - target
        };
        if residual == 0. {
            return Ok(x.copysign(m));
        }
        if residual > 0. {
            upper = x;
        } else {
            lower = x;
        }
        if upper - lower <= 4. * f64::EPSILON * x.max(1e-12) {
            return Ok(((lower + upper) / 2.).copysign(m));
        }
    }
    Err(invalid("Kepler equation did not converge"))
}

pub fn orbital_state(elements: OrbitalElements, jd: f64) -> io::Result<OrbitalState> {
    let OrbitalElements {
        epoch_jd,
        semi_major_axis_au: a,
        eccentricity: e,
        mean_anomaly_deg,
        mean_motion_deg_per_day: n,
        ascending_node_deg,
        inclination_deg,
        argument_of_periapsis_deg,
    } = elements;
    if ![
        epoch_jd,
        a,
        e,
        mean_anomaly_deg,
        n,
        ascending_node_deg,
        inclination_deg,
        argument_of_periapsis_deg,
        jd,
    ]
    .iter()
    .all(|v| v.is_finite())
        || a <= 0.
        || e < 0.
        || e == 1.
    {
        return Err(invalid(
            "require finite orbital elements, a>0 and e>=0, e!=1",
        ));
    }
    let mean = n.mul_add(jd - epoch_jd, mean_anomaly_deg).to_radians();
    if !mean.is_finite() {
        return Err(invalid("anomaly overflow"));
    }
    let anomaly = eccentric_anomaly(e, mean)?;
    let (position, velocity) = if e < 1. {
        let (s, c) = anomaly.sin_cos();
        let minor = ((1. - e) * (1. + e)).sqrt();
        let rate = n.to_radians() / (1. - e * c);
        (
            [a * (c - e), a * minor * s, 0.],
            [-a * s * rate, a * minor * c * rate, 0.],
        )
    } else {
        let (s, c) = (anomaly.sinh(), anomaly.cosh());
        let minor = (e - 1.).sqrt() * (e + 1.).sqrt();
        let rate = n.to_radians() / (e * c - 1.);
        (
            [a * (e - c), a * minor * s, 0.],
            [-a * s * rate, a * minor * c * rate, 0.],
        )
    };
    let rotate = |v| {
        rotate_z(
            rotate_x(rotate_z(v, argument_of_periapsis_deg), inclination_deg),
            ascending_node_deg,
        )
    };
    let result = OrbitalState {
        position_au: rotate(position),
        velocity_au_per_day: rotate(velocity),
    };
    if !result
        .position_au
        .iter()
        .chain(&result.velocity_au_per_day)
        .all(|v| v.is_finite())
    {
        return Err(invalid("orbital state overflow"));
    }
    Ok(result)
}
const ELEMENTS: [[f64; 6]; 9] = [
    [0.38710, 0.20563, 7.0050, 48.3665, 77.5028, 55.4500],
    [0.72333, 0.00677, 3.3947, 76.7069, 131.6058, 137.1556],
    [1.00000, 0.01671, 0.0000, 0.0000, 102.9889, 100.2431],
    [1.52368, 0.09340, 1.8497, 49.5812, 336.1155, 209.5529],
    [5.20260, 0.04850, 1.3031, 100.4950, 14.3796, 125.4198],
    [9.55491, 0.05554, 2.4888, 113.6918, 93.1161, 86.7744],
    [19.21845, 0.04638, 0.7732, 74.0216, 173.0499, 326.9480],
    [30.11039, 0.00946, 1.7697, 131.8171, 48.1631, 310.9437],
    [39.54044, 0.24901, 17.1449, 110.3587, 224.1833, 242.8665],
];
/// Mercury = 0 through Pluto = 8 in Mitaka source order.
pub fn mean_planet_elements(index: usize) -> io::Result<OrbitalElements> {
    let &[a, e, inclination, node, perihelion, longitude] = ELEMENTS
        .get(index)
        .ok_or_else(|| invalid("unknown planet index"))?;
    Ok(OrbitalElements {
        epoch_jd: 2452640.5,
        semi_major_axis_au: a,
        eccentricity: e,
        mean_anomaly_deg: longitude - perihelion,
        mean_motion_deg_per_day: 0.985607669 / a.powf(1.5),
        ascending_node_deg: node,
        inclination_deg: inclination,
        argument_of_periapsis_deg: perihelion - node,
    })
}
pub fn mean_planet_state(index: usize, jd: f64) -> io::Result<OrbitalState> {
    orbital_state(mean_planet_elements(index)?, jd)
}
