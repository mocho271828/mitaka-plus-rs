/*
Copyright (c) 2007-2014   Tsunehiko Kato

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
//! Official topography.cpp body-fixed geometry, following Mitaka_plus f9fe624 terrain_mesh.cpp.
use super::invalid;
use std::io;
pub type Vec3 = [f64; 3];
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(u8)]
pub enum Amplification {
    #[default]
    Actual = 1,
    Double = 2,
    Five = 3,
    Ten = 4,
    Twenty = 5,
}
impl Amplification {
    pub const ALL: [Self; 5] = [
        Self::Actual,
        Self::Double,
        Self::Five,
        Self::Ten,
        Self::Twenty,
    ];
    pub fn stage(self) -> u8 {
        self as u8
    }
    pub fn from_stage(stage: u64) -> io::Result<Self> {
        Self::ALL
            .get(
                stage
                    .checked_sub(1)
                    .and_then(|v| usize::try_from(v).ok())
                    .unwrap_or(usize::MAX),
            )
            .copied()
            .ok_or_else(|| invalid("terrain amplification stage must be 1..5"))
    }
    pub fn factor(self) -> f64 {
        match self {
            Self::Actual => 1.,
            Self::Double => 2.,
            Self::Five => 5.,
            Self::Ten => 10.,
            Self::Twenty => 20.,
        }
    }
}
#[derive(Clone, Copy, Debug)]
pub struct Bounds {
    pub west: f64,
    pub east: f64,
    pub north: f64,
    pub south: f64,
    pub levels: usize,
}
pub fn unit_direction(lon: f64, lat: f64) -> Vec3 {
    let (sl, cl) = lat.to_radians().sin_cos();
    let (so, co) = (lon % 360.).to_radians().sin_cos();
    [cl * co, cl * so, sl]
}
fn dot(a: Vec3, b: Vec3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn sub(a: Vec3, b: Vec3) -> Vec3 {
    std::array::from_fn(|i| a[i] - b[i])
}
fn cross(a: Vec3, b: Vec3) -> Vec3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn norm(v: Vec3) -> f64 {
    v[0].hypot(v[1]).hypot(v[2])
}
impl Bounds {
    fn middle(self) -> (f64, f64) {
        (
            0.5 * self.west + 0.5 * self.east,
            0.5 * self.north + 0.5 * self.south,
        )
    }
    /// Conservative spherical cap for the entire patch plus a requested field radius.
    pub fn near(self, ray: Vec3, angular_radius: f64) -> bool {
        let (lon, lat) = self.middle();
        let extent = ((self.east - self.west) + (self.north - self.south)).to_radians() * 0.5
            + angular_radius;
        extent >= std::f64::consts::FRAC_PI_2
            || dot(unit_direction(lon, lat), ray) >= extent.cos() - 1e-12
    }
    pub fn level(self, eye: Vec3, low_resolution: bool) -> io::Result<usize> {
        if !norm(eye).is_finite()
            || ![self.west, self.east, self.north, self.south]
                .iter()
                .all(|v| v.is_finite())
            || self.east <= self.west
            || self.east - self.west > 360.
            || self.north > 90.
            || self.south < -90.
            || self.north <= self.south
            || !(1..=10).contains(&self.levels)
        {
            return Err(invalid("invalid terrain view or level count"));
        }
        let (lon, lat) = self.middle();
        let distance = norm(sub(unit_direction(lon, lat), eye));
        let diameter = 2.
            * ((self.east - self.west) * lat.to_radians().cos())
                .max(self.north - self.south)
                .to_radians();
        let level = (0..self.levels)
            .rev()
            .find(|&l| diameter / distance > 0.13_f64.to_radians() * (1u32 << l) as f64)
            .unwrap_or(0);
        Ok(level.saturating_sub(usize::from(low_resolution)))
    }
}
pub struct GridWindow<'a> {
    pub patch: usize,
    pub level: usize,
    pub columns: usize,
    pub rows: usize,
    pub bounds: Bounds,
    pub radius_m: f64,
    pub unit_m: f64,
    pub(super) bytes: &'a [u8],
}
impl GridWindow<'_> {
    fn value(&self, col: isize, row: isize) -> f64 {
        let index = 2 * ((row + 1) as usize * (self.columns + 3) + (col + 1) as usize);
        self.unit_m
            * f64::from(i16::from_le_bytes([
                self.bytes[index],
                self.bytes[index + 1],
            ]))
    }
    pub fn elevation_m(&self, lon: f64, lat: f64) -> io::Result<f64> {
        let b = self.bounds;
        if !(b.west..=b.east).contains(&lon) || !(b.south..=b.north).contains(&lat) {
            return Err(invalid("terrain location outside patch"));
        }
        let x = (lon - b.west) / (b.east - b.west) * self.columns as f64;
        let y = (b.north - lat) / (b.north - b.south) * self.rows as f64;
        let col = (x as usize).min(self.columns - 1) as isize;
        let row = (y as usize).min(self.rows - 1) as isize;
        let dx = x - col as f64;
        let dy = y - row as f64;
        let top = self.value(col, row) * (1. - dx) + self.value(col + 1, row) * dx;
        let bot = self.value(col, row + 1) * (1. - dx) + self.value(col + 1, row + 1) * dx;
        Ok(top * (1. - dy) + bot * dy)
    }
    fn point(&self, col: isize, row: isize, factor: f64) -> io::Result<Vec3> {
        let b = self.bounds;
        let lon = b.west + (b.east - b.west) * col as f64 / self.columns as f64;
        let lat = b.north + (b.south - b.north) * row as f64 / self.rows as f64;
        let ratio = 1. + self.value(col, row) / self.radius_m * factor;
        if !ratio.is_finite() || ratio <= 0. {
            return Err(invalid("nonpositive displaced terrain radius"));
        }
        Ok(unit_direction(lon, lat).map(|v| v * ratio))
    }
    /// Source grid with its original diagonal and central-difference halo normals.
    /// Positions are in datum-radius units. UV is top-origin and remains unwrapped.
    pub fn mesh(&self, mode: Amplification) -> io::Result<Mesh> {
        let factor = mode.factor();
        let stride = self.columns + 3;
        let mut halo = Vec::with_capacity(stride * (self.rows + 3));
        for row in -1..=self.rows as isize + 1 {
            for col in -1..=self.columns as isize + 1 {
                halo.push(self.point(col, row, factor)?);
            }
        }
        let mut vertices = Vec::with_capacity((self.columns + 1) * (self.rows + 1));
        for row in 0..=self.rows {
            for col in 0..=self.columns {
                let k = (row + 1) * stride + col + 1;
                let position = halo[k];
                let normal = if (row == 0 && self.bounds.north == 90.)
                    || (row == self.rows && self.bounds.south == -90.)
                {
                    position
                } else {
                    cross(
                        sub(halo[k + stride], halo[k - stride]),
                        sub(halo[k + 1], halo[k - 1]),
                    )
                };
                let n = norm(normal);
                if n == 0. || !n.is_finite() {
                    return Err(invalid("degenerate terrain normal"));
                }
                let lon = self.bounds.west
                    + (self.bounds.east - self.bounds.west) * col as f64 / self.columns as f64;
                let lat = self.bounds.north
                    + (self.bounds.south - self.bounds.north) * row as f64 / self.rows as f64;
                vertices.push(Vertex {
                    position,
                    normal: normal.map(|v| v / n),
                    uv: [lon / 360., 0.5 - lat / 180.],
                    elevation_m: self.value(col as isize, row as isize),
                });
            }
        }
        Ok(Mesh {
            columns: self.columns,
            rows: self.rows,
            vertices,
        })
    }
}
#[derive(Clone, Copy, Debug)]
pub struct Vertex {
    pub position: Vec3,
    pub normal: Vec3,
    pub uv: [f64; 2],
    pub elevation_m: f64,
}
#[derive(Debug)]
pub struct Mesh {
    pub columns: usize,
    pub rows: usize,
    pub vertices: Vec<Vertex>,
}
impl Mesh {
    pub fn triangles(&self) -> impl Iterator<Item = [usize; 3]> + '_ {
        (0..self.rows).flat_map(move |r| {
            (0..self.columns).flat_map(move |c| {
                let a = r * (self.columns + 1) + c;
                let b = a + self.columns + 1;
                [[a, b, a + 1], [a + 1, b, b + 1]]
            })
        })
    }
    /// Outermost intersection of a radial unit ray with the drawn terrain triangles.
    pub fn radial_intersection(&self, ray: Vec3) -> Option<f64> {
        self.triangles()
            .filter_map(|tri| triangle_hit(ray, tri.map(|idx| self.vertices[idx].position)))
            .max_by(f64::total_cmp)
    }
}
fn triangle_hit(ray: Vec3, [a, b, c]: [Vec3; 3]) -> Option<f64> {
    let ab = sub(b, a);
    let ac = sub(c, a);
    let normal = cross(ab, ac);
    let n2 = dot(normal, normal);
    let denom = dot(normal, ray);
    if n2 <= 0. || denom.abs() <= n2.sqrt() * 1e-14 {
        return None;
    }
    let radius = dot(normal, a) / denom;
    if !radius.is_finite() || radius <= 0. {
        return None;
    }
    let p = sub(ray.map(|v| v * radius), a);
    let u = dot(cross(p, ac), normal) / n2;
    let v = dot(cross(ab, p), normal) / n2;
    (u >= -1e-9 && v >= -1e-9 && u + v <= 1. + 1e-9).then_some(radius)
}
