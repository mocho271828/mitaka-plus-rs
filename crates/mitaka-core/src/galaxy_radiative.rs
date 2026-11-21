/*
Copyright (c) 2007   Tsunehiko Kato  and  4D2U Project, National Astronomical Observatory of Japan

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
//! Milky Way emission and extinction, following Mitaka 1.3.1 and Mitaka_plus
//! f9fe624 our_galaxy_radiative.cpp. All input coordinates are galactocentric pc.
use crate::galaxy::OurGalaxyModel;
use std::io;
pub type Vector = [f32; 3];
const FLATTENING: f32 = 0.3;
const RADIUS: f32 = 15000.;
const BASE: f32 = 0.004;
const EXTINCTION: Vector = [1., 1.33, 1.78];
const THIN: Vector = [0.94128734, 0.9976225, 0.99999994];
const THICK: Vector = [0.99999994, 0.9601821, 0.7210822];
fn invalid(reason: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, reason)
}
fn dot(a: Vector, b: Vector) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn norm(v: Vector) -> f32 {
    v[0].hypot(v[1]).hypot(v[2])
}
fn flatten([x, y, z]: Vector) -> Vector {
    [x, y, z / FLATTENING]
}
fn advance(origin: Vector, dir: Vector, distance: f32) -> Vector {
    std::array::from_fn(|i| origin[i] + dir[i] * distance)
}
fn validate(point: Vector, step: f32) -> io::Result<()> {
    if !(1. ..=1000.).contains(&step) || point.iter().any(|v| !v.is_finite() || v.abs() > 1e7) {
        return Err(invalid("invalid galactocentric position or ray step"));
    }
    Ok(())
}
fn span(origin: Vector, dir: Vector) -> Option<[f32; 2]> {
    let b = dot(dir, origin);
    let c = dot(origin, origin) - RADIUS * RADIUS;
    let d = b * b - c;
    (d > 0.).then(|| [(-b - d.sqrt()).max(0.), -b + d.sqrt()])
}
fn exp_approx(x: f32) -> f64 {
    let x = f64::from(x);
    let x2 = x * x;
    let a = 1. + 0.2507213 * x + 0.0292732 * x2 + 0.0038278 * (x * x2);
    let a2 = a * a;
    1. / (a2 * a2)
}
fn transmission(tau: f32) -> Vector {
    let optical = (tau * 100.).trunc().min(500.) / 100.;
    EXTINCTION.map(|ratio| (-ratio * optical).exp())
}
fn bulge(radius: f32) -> io::Result<f32> {
    let idx = (radius * (3000. / 13500.)).trunc().min(2999.);
    if idx == 0. {
        return Err(invalid("singular Milky Way bulge table entry"));
    }
    let r3 =
        (0.0011 * f64::from((7.669 * (8000_f32 / 2700.).powf(0.25) - 8000. / 3800.).exp())) as f32;
    let s = 5. * idx / 2999.;
    Ok((f64::from(r3 * (s / 8000. * 2700.).powf(-7. / 8.))
        * (-7.669 * f64::from(s.powf(0.25))).exp()) as f32)
}
#[derive(Clone, Copy, Debug)]
pub struct Ray {
    pub intensity: Vector,
    pub transmittance: Vector,
    pub optical_depth: f32,
    pub steps: usize,
}
impl Default for Ray {
    fn default() -> Self {
        Self {
            intensity: [0.; 3],
            transmittance: [1.; 3],
            optical_depth: 0.,
            steps: 0,
        }
    }
}
impl Ray {
    pub fn compose(self, background: Vector, alpha: f32) -> io::Result<Vector> {
        if !(0. ..=1.).contains(&alpha)
            || background
                .iter()
                .chain(&self.intensity)
                .chain(&self.transmittance)
                .any(|v| !v.is_finite())
        {
            return Err(invalid("invalid Milky Way composition input"));
        }
        Ok(std::array::from_fn(|i| {
            background[i] * ((1. - alpha) + alpha * self.transmittance[i])
                + alpha * self.intensity[i]
        }))
    }
}
#[derive(Debug)]
pub struct PointExtinction {
    pub optical_depth: f32,
    pub transmittance: Vector,
    pub steps: usize,
    pub visible: bool,
}
impl Default for PointExtinction {
    fn default() -> Self {
        Self {
            optical_depth: 0.,
            transmittance: [1.; 3],
            steps: 0,
            visible: true,
        }
    }
}
#[derive(Debug)]
pub struct RadiativeModel {
    maps: [Vec<u8>; 2],
    transform: [f32; 2],
}
impl RadiativeModel {
    pub fn from_model(source: &OurGalaxyModel) -> io::Result<Self> {
        let mut maps = [Vec::new(), Vec::new()];
        for (i, map) in source.maps.iter().enumerate() {
            if (map.width, map.height, map.values.len()) != (512, 512, 512 * 512) {
                return Err(invalid("Milky Way maps must be 512 by 512"));
            }
            maps[i].reserve(map.values.len());
            for &v in &map.values {
                if !v.is_finite() || v < 0. {
                    return Err(invalid("invalid Milky Way map sample"));
                }
                let v = if i == 0 { v * 0.7 + 0.3 } else { v };
                maps[i].push((255. * v.min(1.)) as u8);
            }
        }
        let scale = 138.5_f32.hypot(68.) / 8000.;
        let (s, c) = 68.0_f32.atan2(138.5).sin_cos();
        Ok(Self {
            maps,
            transform: [scale * c, scale * s],
        })
    }
    pub fn modulation(&self, x: f32, y: f32) -> io::Result<[f32; 2]> {
        if !x.is_finite() || !y.is_finite() {
            return Err(invalid("invalid modulation position"));
        }
        let [c, s] = self.transform;
        let (px, py) = (x * c - y * s + 256.5, x * s + y * c + 295.5);
        if !(0. ..512.).contains(&px) || !(0. ..512.).contains(&py) {
            return Ok([0.3, BASE]);
        }
        let i = py as usize * 512 + px as usize;
        Ok([
            f32::from(self.maps[0][i]) / 255.,
            BASE + BASE * 0.6 * f32::from(self.maps[1][i]) / 255.,
        ])
    }
    pub fn trace(&self, origin_pc: Vector, direction: Vector, step_pc: f32) -> io::Result<Ray> {
        validate(origin_pc, step_pc)?;
        let length = norm(direction);
        if !length.is_finite() || length == 0. {
            return Err(invalid("invalid ray direction"));
        }
        let unit = direction.map(|v| v / length);
        let scaled = flatten(unit);
        let scale = norm(scaled);
        let dir = scaled.map(|v| v / scale);
        let origin = flatten(origin_pc);
        let mut out = Ray::default();
        let Some([start, end]) = span(origin, dir) else {
            return Ok(out);
        };
        if end <= start {
            return Ok(out);
        }
        let mut p = advance(origin, dir, start);
        p[2] *= FLATTENING;
        let limit = (end - start) / scale;
        let cs = unit[2].abs().max(0.3);
        let ds1 = step_pc / 1.29 / cs;
        let ds2 = 0.29 / 1.29 / cs;
        let mut ds = ds1 + p[2].abs() * ds2;
        let mut travelled = 0.;
        while travelled < limit {
            out.steps += 1;
            if out.steps > 100000 {
                return Err(invalid("ray integration step limit exceeded"));
            }
            let m = self.modulation(p[0], p[1])?;
            let rho = p[0].hypot(p[1]);
            let z = p[2].abs();
            let thin = (f64::from(ds * m[0]) * exp_approx(rho / 3800. + z / 350.)) as f32;
            let thick = (f64::from(ds)
                * (f64::from(0.019_f32) * exp_approx(rho / 3800. + z / 900.)
                    + f64::from(bulge(rho.hypot(z / 0.84))?))) as f32;
            let t = transmission(out.optical_depth);
            for i in 0..3 {
                out.intensity[i] += (THIN[i] * thin + THICK[i] * thick) * t[i];
            }
            out.optical_depth = (f64::from(out.optical_depth)
                + f64::from(ds * m[1]) * exp_approx(rho / 3800. + z / 186.))
                as f32;
            if out.optical_depth > 4. {
                break;
            }
            p = advance(p, unit, ds);
            travelled += ds;
            ds = ds1 + ds2 * z;
        }
        out.intensity = out.intensity.map(|v| (v * 0.0035).min(1.));
        out.transmittance = transmission(out.optical_depth);
        Ok(out)
    }
}
/// The point-source branch uses the ellipsoidal z and its own dust approximation.
pub fn point_extinction(
    observer: Vector,
    point: Vector,
    step_pc: f32,
) -> io::Result<PointExtinction> {
    validate(observer, step_pc)?;
    validate(point, step_pc)?;
    let delta = std::array::from_fn(|i| point[i] - observer[i]);
    let len = norm(delta);
    let mut out = PointExtinction::default();
    if len == 0. {
        return Ok(out);
    }
    let ds = flatten(delta);
    let scaled = norm(ds);
    let dir = ds.map(|v| v / scaled);
    let origin = flatten(observer);
    let Some([start, end]) = span(origin, dir) else {
        return Ok(out);
    };
    let length = end.min(scaled) - start;
    if length <= 0. {
        return Ok(out);
    }
    let mut p = advance(origin, dir, start);
    let n = delta.map(|v| v / len);
    let cs = n[2].abs().max(0.3);
    let ds1 = step_pc / 1.27 / cs;
    let ds2 = 0.27 / 1.27 / cs;
    let mut step = ds1 + p[2].abs() * ds2;
    let mut travelled = 0.;
    while travelled < length {
        out.steps += 1;
        if out.steps > 100000 {
            return Err(invalid("point extinction step limit exceeded"));
        }
        let rho = p[0].hypot(p[1]);
        out.optical_depth = (f64::from(out.optical_depth)
            + f64::from(step * BASE) * exp_approx(rho / 3800. + p[2].abs() / 186.))
            as f32;
        if out.optical_depth > 3. {
            break;
        }
        p = advance(p, n, step);
        travelled += step;
        step = ds1 + p[2].abs() * ds2;
    }
    out.visible = out.optical_depth <= 2.9;
    out.transmittance = transmission(out.optical_depth);
    Ok(out)
}
