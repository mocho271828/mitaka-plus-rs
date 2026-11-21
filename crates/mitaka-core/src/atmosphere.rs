/*
Copyright (c) 2007-2015   Tsunehiko Kato  and  4D2U Project, National Astronomical Observatory of Japan

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
//! Mitaka 1.3.1 scalar single/multiple scattering, via Mitaka_plus f9fe624.
//! Intensities are additive RGB. Positions and distances use meters, directions
//! must already be unit vectors in the same orthogonal frame.
use crate::{
    astronomy::Vector3,
    binary::{Reader, at_path, read_bounded},
    catalog::CatalogLimits,
};
use std::{f64::consts::PI, io, path::Path, sync::LazyLock};
pub type Rgb = [f32; 3];
#[derive(Clone, Copy, Debug)]
pub struct Parameters {
    pub ground_radius_m: f64,
    pub outer_radius_m: f64,
    pub scale_height_m: f64,
    pub surface_number_density_per_m3: f64,
    pub incident_intensity: f64,
}
#[derive(Clone, Copy, Debug)]
pub struct Ray {
    pub observer_position_m: Vector3,
    pub view_direction: Vector3,
    pub sun_direction: Vector3,
    pub maximum_distance_m: Option<f64>,
}
fn invalid(m: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, m)
}
fn dot(a: Vector3, b: Vector3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn mul(a: Vector3, x: f64) -> Vector3 {
    a.map(|v| v * x)
}
fn add(a: Vector3, b: Vector3) -> Vector3 {
    std::array::from_fn(|i| a[i] + b[i])
}
fn length(a: Vector3) -> f64 {
    a[0].hypot(a[1]).hypot(a[2])
}
fn attenuation(x: f64) -> f64 {
    let x2 = x * x;
    let x3 = x * x2;
    let a = 1. + 0.2507213 * x + 0.0292732 * x2 + 0.0038278 * x3;
    let a2 = a * a;
    1. / (a2 * a2)
}
fn coefficients(density: f64) -> Vector3 {
    let ns = 2.5469e25;
    let dr = 0.031;
    let ns1 = 295e-6;
    let sigma = 32. * PI * PI * PI / 3. * (ns1 * ns1) / (ns * ns) * (6. + 3. * dr) / (6. - 7. * dr);
    [700e-9f32, 546.1e-9, 435.8e-9].map(|l| density * (sigma / f64::from(l).powi(4)))
}
fn checked(rgb: Rgb) -> io::Result<Rgb> {
    if rgb.iter().all(|x| x.is_finite() && *x >= 0.) {
        Ok(rgb)
    } else {
        Err(invalid("invalid atmosphere intensity"))
    }
}
impl Ray {
    fn validate(self, ground: f64) -> io::Result<()> {
        let Self {
            observer_position_m: p,
            view_direction: v,
            sun_direction: s,
            maximum_distance_m: limit,
        } = self;
        if !p.into_iter().chain(v).chain(s).all(f64::is_finite)
            || limit.is_some_and(|d| !d.is_finite() || d < 0.)
            || (length(v) - 1.).abs() > 2e-6
            || (length(s) - 1.).abs() > 2e-6
            || length(p) < ground
            || !dot(p, p).is_finite()
        {
            return Err(invalid("invalid observer, ray direction or distance cap"));
        }
        Ok(())
    }
}
impl Parameters {
    pub fn earth() -> Self {
        Self {
            ground_radius_m: 6378136.6,
            outer_radius_m: 6378136.6 + 6. * 8000.,
            scale_height_m: 8000.,
            surface_number_density_per_m3: 2.69e25,
            incident_intensity: 6.,
        }
    }
    pub fn mars(radius_m: f64) -> io::Result<Self> {
        let obj = Self {
            ground_radius_m: radius_m,
            outer_radius_m: radius_m + 8. * 11000.,
            scale_height_m: 11000.,
            surface_number_density_per_m3: 4e23,
            incident_intensity: 6.,
        };
        obj.validate()?;
        Ok(obj)
    }
    fn validate(self) -> io::Result<()> {
        let Self {
            ground_radius_m: g,
            outer_radius_m: o,
            scale_height_m: h,
            surface_number_density_per_m3: n,
            incident_intensity: i,
        } = self;
        if ![g, o, h, n, i].into_iter().all(f64::is_finite)
            || g <= 0.
            || o <= g
            || h <= 0.
            || n < 0.
            || i < 0.
            || g * g == 0.
            || !(o * o).is_finite()
            || (g / h).powi(2) == 0.
            || !(o / h).powi(2).is_finite()
        {
            return Err(invalid("invalid atmosphere geometry or parameters"));
        }
        Ok(())
    }
}
fn integrate(
    p: Parameters,
    ray: Ray,
    coeff: Vector3,
    start: f64,
    end: f64,
    n: usize,
) -> io::Result<Rgb> {
    let height = p.scale_height_m;
    let ground = p.ground_radius_m * 0.9999 / height;
    let outer = p.outer_radius_m / height;
    let step = mul(ray.view_direction, (end - start) / (n - 1) as f64 / height);
    let mut point = add(
        mul(ray.view_direction, start / height),
        mul(ray.observer_position_m, 1. / height),
    );
    let mut intensity = [0f32; 3];
    let mut view_depth = 0.;
    for i in 0..n {
        let squared = dot(point, point);
        if !squared.is_finite() {
            return Err(invalid("scaled atmospheric ray overflows"));
        }
        let density = attenuation(squared.sqrt() - ground);
        view_depth += density;
        let b = dot(point, ray.sun_direction);
        let discr = b * b - squared + outer * outer;
        let sun_distance = if discr <= 0. { -b } else { -b + discr.sqrt() };
        let ns = n - i + 1;
        let delta = mul(ray.sun_direction, sun_distance / (ns - 1) as f64);
        let mut sun_point = point;
        let mut sun_depth = 0.;
        for _ in 0..ns {
            sun_depth += attenuation(dot(sun_point, sun_point).sqrt() - ground);
            sun_point = add(sun_point, delta);
        }
        let depth = view_depth * ((end - start) / n as f64)
            + sun_depth * ((sun_distance * height) / ns as f64);
        for k in 0..3 {
            let tau = coeff[k] * depth;
            if !tau.is_finite() {
                return Err(invalid("atmosphere optical depth overflow"));
            }
            intensity[k] = (f64::from(intensity[k]) + density * attenuation(tau)) as f32;
        }
        point = add(point, step);
    }
    let step_m = ((end - start) / (n - 1) as f64) as f32;
    checked(std::array::from_fn(|k| {
        (f64::from(intensity[k]) * (coeff[k] * f64::from(step_m))) as f32
    }))
}
pub fn single_scattering(p: Parameters, ray: Ray, samples: usize) -> io::Result<Rgb> {
    p.validate()?;
    ray.validate(p.ground_radius_m)?;
    if !(2..=4096).contains(&samples) {
        return Err(invalid("sample count outside 2..4096"));
    }
    if p.incident_intensity == 0. || p.surface_number_density_per_m3 == 0. {
        return Ok([0.; 3]);
    }
    let (origin, v, s) = (
        ray.observer_position_m,
        ray.view_direction,
        ray.sun_direction,
    );
    let b = dot(v, origin);
    let d0 = b * b - dot(origin, origin);
    let shell = d0 + p.outer_radius_m * p.outer_radius_m;
    if shell <= 0. {
        return Ok([0.; 3]);
    }
    let mut start = 0f64.max(-b - shell.sqrt());
    let mut end = -b + shell.sqrt();
    if let Some(limit) = ray.maximum_distance_m {
        end = end.min(limit);
    }
    if end <= start {
        return Ok([0.; 3]);
    }
    let ground = p.ground_radius_m * 0.9999;
    let discr = d0 + ground * ground;
    if discr >= 0. {
        let (near, far) = (-b - discr.sqrt(), -b + discr.sqrt());
        if start < far && start <= near && end > near {
            end = near;
        }
    }
    let perp = |a: Vector3| add(a, mul(s, -dot(a, s)));
    let (vp, op) = (perp(v), perp(origin));
    let a = dot(vp, vp);
    let sb = dot(vp, op);
    let sd = sb * sb - a * (dot(op, op) - ground * ground);
    let mut second = None;
    if sd > 0. {
        let (near, far) = ((-sb - sd.sqrt()) / a, (-sb + sd.sqrt()) / a);
        if start < far {
            let el = dot(v, s);
            let os = dot(origin, s);
            if start >= near {
                if start * el + os < 0. {
                    if end <= far {
                        return Ok([0.; 3]);
                    }
                    start = far;
                }
            } else if near * el + os < 0. && end > near {
                if end > far {
                    second = Some((far, end));
                }
                end = near;
            }
        }
    }
    let coeff = coefficients(p.surface_number_density_per_m3);
    let mut rgb = integrate(p, ray, coeff, start, end, samples)?;
    if let Some((start, end)) = second {
        let rhs = integrate(p, ray, coeff, start, end, samples)?;
        for k in 0..3 {
            rgb[k] += rhs[k];
        }
    }
    let cos = dot(v, mul(s, -1.));
    #[allow(
        clippy::approx_constant,
        reason = "retain the phase constant of the original atmosphere model"
    )]
    let factor = 3. / (16. * 3.141592654) * (1. + cos * cos) * p.incident_intensity;
    checked(rgb.map(|v| (f64::from(v) * factor) as f32))
}
pub fn compose_display(single: Rgb, multiple: Rgb, intensity: f64) -> io::Result<(Rgb, f32)> {
    checked(single)?;
    checked(multiple)?;
    if !intensity.is_finite() || intensity < 0. {
        return Err(invalid("invalid display intensity"));
    }
    let rgb = checked(std::array::from_fn(|k| {
        single[k] + intensity as f32 * multiple[k]
    }))?;
    Ok((rgb, (4. * rgb.into_iter().fold(0., f32::max)).min(1.)))
}

#[derive(Debug)]
pub struct EmissionMap {
    ground: f64,
    outer: f64,
    height: f64,
    density: f64,
    max_angle: f64,
    radii: Vec<f64>,
    angles: usize,
    nodes: Vec<Rgb>,
}
impl EmissionMap {
    pub fn load(path: &Path) -> io::Result<Self> {
        let l = CatalogLimits::default();
        at_path(
            path,
            read_bounded(path, l.max_file_bytes).and_then(|b| Self::from_bytes(&b, l)),
        )
    }
    pub fn from_bytes(bytes: &[u8], limits: CatalogLimits) -> io::Result<Self> {
        let mut r = Reader::new(bytes);
        let radial = r.i32()?;
        let angles = r.i32()?;
        if radial < 2
            || angles < 2
            || bytes.len() as u64 > limits.max_file_bytes
            || radial as usize > limits.max_records / angles as usize
        {
            return Err(r.invalid("invalid emission grid dimensions"));
        }
        let (nr, angles) = (radial as usize, angles as usize);
        let count = nr * angles;
        if bytes.len().checked_sub(48) != count.checked_mul(12) {
            return Err(r.invalid("expected exact 48-byte header and RGB32 grid"));
        }
        let mut real = || Ok::<f64, io::Error>(f64::from_le_bytes(r.array()?));
        let (ground, outer, height, density, max_angle) =
            (real()?, real()?, real()?, real()?, real()?);
        if ![ground, outer, height, density, max_angle]
            .into_iter()
            .all(f64::is_finite)
            || ground <= 0.
            || outer <= ground
            || height <= 0.
            || density < 0.
            || !(0. < max_angle && max_angle <= PI)
            || (outer * outer).is_infinite()
            || ground * ground == 0.
            || max_angle / (angles - 1) as f64 == 0.
        {
            return Err(r.invalid("invalid emission map metadata"));
        }
        let radii = (0..nr)
            .map(|i| ground + (outer - ground) * i as f64 / (nr - 1) as f64)
            .collect::<Vec<_>>();
        if !radii.windows(2).all(|p| p[1].is_finite() && p[1] > p[0]) {
            return Err(r.invalid("invalid emission radial spacing"));
        }
        let nodes = (0..count)
            .map(|_| {
                let node = [r.f32()?, r.f32()?, r.f32()?];
                if node.iter().any(|&v| v < 0.) {
                    return Err(r.invalid("negative emission"));
                }
                Ok(node)
            })
            .collect::<io::Result<_>>()?;
        Ok(Self {
            ground,
            outer,
            height,
            density,
            max_angle,
            radii,
            angles,
            nodes,
        })
    }
    pub fn dimensions(&self) -> [usize; 2] {
        [self.radii.len(), self.angles]
    }
    pub fn ground_radius_m(&self) -> f64 {
        self.ground
    }
    pub fn outer_radius_m(&self) -> f64 {
        self.outer
    }
}
static ACOS: LazyLock<[f64; 500]> =
    LazyLock::new(|| std::array::from_fn(|i| (i as f64 / 499.).acos()));
pub fn multiple_scattering(map: &EmissionMap, ray: Ray) -> io::Result<Rgb> {
    ray.validate(map.ground)?;
    let radius = dot(ray.observer_position_m, ray.observer_position_m).sqrt();
    let b = dot(ray.observer_position_m, ray.view_direction);
    if radius <= map.ground && b < 0. {
        return Ok([0.; 3]);
    }
    let d0 = b * b - radius * radius;
    let od = d0 + map.outer * map.outer;
    if !od.is_finite() {
        return Err(invalid("atmosphere shell geometry overflow"));
    }
    if od < 0. || -b + od.sqrt() <= 0. {
        return Ok([0.; 3]);
    }
    let radii = &map.radii;
    let mut index = radii.partition_point(|&v| v <= radius).saturating_sub(1);
    let mut t = 0.;
    if index == radii.len() - 1 {
        t = -b - od.sqrt();
        index -= 1;
    }
    if ray.maximum_distance_m.is_some_and(|end| t >= end) {
        return Ok([0.; 3]);
    }
    let coeff = coefficients(map.density);
    let da = map.max_angle / (map.angles - 1) as f64;
    let mut depth = [0.; 3];
    let mut rgb = [0f32; 3];
    let mut advance = |i: usize, next: f64| -> io::Result<bool> {
        let radius = radii[i];
        let point = add(mul(ray.view_direction, next), ray.observer_position_m);
        let stop = ray.maximum_distance_m.map_or(next, |d| d.min(next));
        let dt = stop - t;
        let half = 0.5 * attenuation((radius - map.ground) / map.height) * dt;
        let tau = coeff.map(|v| v * half);
        for k in 0..3 {
            depth[k] += tau[k];
            if !depth[k].is_finite() {
                return Err(invalid("shell optical depth overflow"));
            }
        }
        let c = dot(mul(point, 1. / radius), ray.sun_direction).clamp(-1., 1.);
        if !c.is_finite() {
            return Err(invalid("shell direction overflow"));
        }
        let table = ACOS[(c.abs() * 499.) as usize];
        let angle = if c >= 0. { table } else { PI - table };
        let a = angle / da;
        if a < (map.angles - 1) as f64 {
            let j = a as usize;
            let hi = ((a - j as f64) * dt) as f32;
            let lo = (dt - f64::from(hi)) as f32;
            let start = i * map.angles + j;
            for k in 0..3 {
                let emitted = lo * map.nodes[start][k] + hi * map.nodes[start + 1][k];
                rgb[k] = (f64::from(rgb[k]) + attenuation(depth[k]) * f64::from(emitted)) as f32;
            }
        }
        for k in 0..3 {
            depth[k] += tau[k];
        }
        t = stop;
        Ok(stop < next || ray.maximum_distance_m.is_some_and(|cap| stop >= cap))
    };
    if b < 0. {
        for i in (0..=index).rev() {
            let d = d0 + radii[i] * radii[i];
            if d < 0. {
                index = i;
                break;
            }
            if advance(i, -b - d.sqrt())? || i == 0 {
                return checked(rgb);
            }
        }
    }
    for (i, radius) in radii.iter().enumerate().skip(index + 1) {
        if advance(i, -b + (d0 + radius * radius).sqrt())? {
            break;
        }
    }
    checked(rgb)
}
