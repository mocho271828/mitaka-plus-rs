/**************************************************************************************************

Copyright (c) 2007, 2015-2016   Tsunehiko Kato  and  4D2U Project, National Astronomical Observatory of Japan

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
//! Eclipse geometry in a single Cartesian frame (usually J2000 ecliptic AU),
//! from Mitaka 1.3.1 and Mitaka_plus f9fe624 eclipse.cpp.
use crate::astronomy::Vector3;
use std::{f64::consts::PI, io};
#[derive(Clone, Copy, Debug)]
pub struct Sphere {
    pub center_au: Vector3,
    pub radius_au: f64,
}
#[derive(Clone, Copy, Debug)]
pub struct Cone {
    pub apex_au: Vector3,
    pub axis: Vector3,
    pub sin_half_angle: f64,
    pub cos_half_angle: f64,
}
#[derive(Clone, Copy, Debug)]
pub struct ShadowCones {
    pub umbra: Cone,
    pub penumbra: Cone,
    pub axis: Vector3,
}
#[derive(Clone, Copy, Debug)]
pub struct SurfaceSample {
    pub obscured_fraction: f64,
    pub sun_radius_rad: f64,
    pub occluder_radius_rad: f64,
    pub separation_rad: f64,
}
fn invalid(s: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, s)
}
fn sub(a: Vector3, b: Vector3) -> Vector3 {
    std::array::from_fn(|i| a[i] - b[i])
}
fn move_along(a: Vector3, v: Vector3, d: f64) -> io::Result<Vector3> {
    let v = std::array::from_fn(|i| a[i] + v[i] * d);
    if v.iter().all(|n: &f64| n.is_finite()) {
        Ok(v)
    } else {
        Err(invalid("eclipse coordinate overflow"))
    }
}
fn dot(a: Vector3, b: Vector3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: Vector3, b: Vector3) -> Vector3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn length(a: Vector3) -> io::Result<f64> {
    let n = a[0].hypot(a[1]).hypot(a[2]);
    if n.is_finite() {
        Ok(n)
    } else {
        Err(invalid("eclipse distance overflow"))
    }
}
fn separated(a: Sphere, b: Sphere) -> io::Result<f64> {
    if ![a, b].iter().all(|s| {
        s.center_au.iter().all(|x| x.is_finite()) && s.radius_au.is_finite() && s.radius_au > 0.
    }) {
        return Err(invalid("invalid eclipse sphere"));
    }
    let d = length(sub(a.center_au, b.center_au))?;
    if d == 0. || a.radius_au / d + b.radius_au / d >= 1. {
        Err(invalid("eclipse spheres overlap or touch"))
    } else {
        Ok(d)
    }
}
/// Area fraction of the solar disk covered by the apparent occluder.
pub fn obscured_fraction(r: f64, d: f64) -> io::Result<f64> {
    if !r.is_finite() || !d.is_finite() || r < 0. || d < 0. {
        return Err(invalid("invalid eclipse angular ratio"));
    }
    if r == 0. {
        return Ok(0.);
    }
    let (large, small) = (r.max(1.), r.min(1.));
    if d >= large && d - large >= small {
        return Ok(0.);
    }
    if d <= large && large - d >= small {
        return Ok(small * small);
    }
    let chord = if r >= 1. {
        if r <= 2. {
            0.5 * (d + (1. - r) / d * (1. + r))
        } else {
            (d - r) * (0.5 + 0.5 * r / d) + 0.5 / d
        }
    } else if r >= 0.5 {
        0.5 * (d + (r - 1.) / d * (r + 1.))
    } else {
        (d - 1.) * (0.5 + 0.5 / d) + 0.5 * r * (r / d)
    };
    if !chord.is_finite() {
        return Err(invalid("eclipse disk chord overflow"));
    }
    let c = (chord / small).clamp(-1., 1.);
    let h = small * ((1. - c) * (1. + c)).sqrt();
    let segment = |r: f64, a: f64| {
        let angle = h.atan2(a);
        if angle < 0.01 {
            let sq = angle * angle;
            let arc = r * angle;
            arc * arc * angle * (2. / 3. + sq * (-2. / 15. + sq * (4. / 315. + sq * (-2. / 2835.))))
        } else {
            r * r * (angle - angle.sin() * angle.cos())
        }
    };
    let value = (segment(small, chord) + segment(large, d - chord)) / PI;
    if !(-1e-12..=small * small + 1e-12).contains(&value) {
        return Err(invalid("disk area overflow"));
    }
    Ok(value.clamp(0., small * small))
}
pub fn surface_sample(sun: Sphere, occluder: Sphere, point: Vector3) -> io::Result<SurfaceSample> {
    separated(sun, occluder)?;
    let to_s = sub(sun.center_au, point);
    let to_o = sub(occluder.center_au, point);
    let ds = length(to_s)?;
    let do_ = length(to_o)?;
    if ds <= sun.radius_au || do_ <= occluder.radius_au {
        return Err(invalid("eclipse point inside sphere"));
    }
    let sr = (sun.radius_au / ds).asin();
    let or = (occluder.radius_au / do_).asin();
    if sr == 0. || or == 0. {
        return Err(invalid("eclipse angular radius underflow"));
    }
    let ns = to_s.map(|v| v / ds);
    let no = to_o.map(|v| v / do_);
    let separation = length(cross(ns, no))?.atan2(dot(ns, no));
    let (r, d) = (or / sr, separation / sr);
    if !r.is_finite() || !d.is_finite() {
        return Err(invalid("eclipse angular ratio overflow"));
    }
    Ok(SurfaceSample {
        obscured_fraction: if do_ >= ds {
            0.
        } else {
            obscured_fraction(r, d)?
        },
        sun_radius_rad: sr,
        occluder_radius_rad: or,
        separation_rad: separation,
    })
}
pub fn occluder_cones(sun: Sphere, occluder: Sphere) -> io::Result<ShadowCones> {
    let d = separated(sun, occluder)?;
    if sun.radius_au <= occluder.radius_au {
        return Err(invalid("solar radius must exceed occluder radius"));
    }
    let axis = sub(occluder.center_au, sun.center_au).map(|v| v / d);
    let make_cone = |s: f64, sign: f64| -> io::Result<Cone> {
        if s <= 0. || s >= 1. {
            return Err(invalid("unsupported cone angle"));
        }
        Ok(Cone {
            apex_au: move_along(occluder.center_au, axis, sign * (occluder.radius_au / s))?,
            axis: axis.map(|v| -sign * v),
            sin_half_angle: s,
            cos_half_angle: ((1. - s) * (1. + s)).sqrt(),
        })
    };
    Ok(ShadowCones {
        umbra: make_cone((sun.radius_au - occluder.radius_au) / d, 1.)?,
        penumbra: make_cone(sun.radius_au / d + occluder.radius_au / d, -1.)?,
        axis,
    })
}
pub fn shadow_cones(
    sun: Sphere,
    occluder: Sphere,
    receiver: Sphere,
) -> io::Result<Option<ShadowCones>> {
    let c = occluder_cones(sun, occluder)?;
    let d = separated(sun, receiver)?;
    separated(occluder, receiver)?;
    if d <= separated(sun, occluder)? {
        return Ok(None);
    }
    let rel = sub(receiver.center_au, c.penumbra.apex_au);
    let distance = length(cross(rel, c.axis))? * c.penumbra.cos_half_angle
        - dot(rel, c.axis) * c.penumbra.sin_half_angle;
    if !distance.is_finite() {
        return Err(invalid("cone distance overflow"));
    }
    Ok((distance <= receiver.radius_au).then_some(c))
}
pub fn shadow_axis_hit(
    sun: Sphere,
    occluder: Sphere,
    receiver: Sphere,
) -> io::Result<Option<Vector3>> {
    let d = separated(sun, occluder)?;
    separated(sun, receiver)?;
    separated(occluder, receiver)?;
    let axis = sub(occluder.center_au, sun.center_au).map(|v| v / d);
    let rel = sub(receiver.center_au, occluder.center_au);
    let (axial, lateral) = (dot(rel, axis), length(cross(rel, axis))?);
    if !axial.is_finite() {
        return Err(invalid("axis distance overflow"));
    }
    if axial < 0. || lateral > receiver.radius_au {
        return Ok(None);
    }
    let ratio = lateral / receiver.radius_au;
    let distance = axial - receiver.radius_au * ((1. - ratio) * (1. + ratio)).sqrt();
    if distance < 0. {
        return Ok(None);
    }
    Ok(Some(move_along(occluder.center_au, axis, distance)?))
}
pub fn shadow_multiplier(fraction: f64, color: [f64; 3]) -> io::Result<[f64; 3]> {
    if !(0.0..=1.).contains(&fraction) || !color.iter().all(|v| (0.0..=1.).contains(v)) {
        return Err(invalid("invalid fraction or shadow color"));
    }
    Ok(color.map(|v| 1. - (1. - v) * fraction))
}
