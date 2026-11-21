/*
Derived from Mitaka 1.3.1: Mitaka/mtk_planet_orientation.cpp.

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
//! Fixed-body axes X (zero longitude), Y (east 90 degrees), Z (north) in the
//! J2000 ecliptic frame. Coefficients from Mitaka 1.3.1 mtk_planet_orientation.cpp.
use crate::astronomy::{
    J2000, J2000_OBLIQUITY_DEG, Vector3, centered, earth_axes, rotate_x, rotate_z, valid_jd,
};
use std::io;
pub fn planet_orientation(index: usize, jd: f64) -> io::Result<[Vector3; 3]> {
    valid_jd(jd)?;
    if index == 2 {
        return Ok(earth_axes(jd).map(|v| rotate_x(v, -J2000_OBLIQUITY_DEG)));
    }
    let d = jd - J2000;
    let t = d / 36525.;
    let sin_deg = |a: f64| centered(a, 360.).to_radians().sin();
    let cos_deg = |a: f64| centered(a, 360.).to_radians().cos();
    let [ra, dec, meridian] = match index {
        0 => {
            let m1 = 174.791086 + 4.092335 * d;
            let m2 = 349.582171 + 8.184670 * d;
            let m3 = 164.373257 + 12.277005 * d;
            let m4 = 339.164343 + 16.369340 * d;
            let m5 = 153.955429 + 20.461675 * d;
            [
                281.0097 - 0.0328 * t,
                61.4143 - 0.0049 * t,
                329.5469 + 6.1385025 * d + 0.00993822 * sin_deg(m1)
                    - 0.00104581 * sin_deg(m2)
                    - 0.00010280 * sin_deg(m3)
                    - 0.00002364 * sin_deg(m4)
                    - 0.00000532 * sin_deg(m5),
            ]
        }
        1 => [272.76, 67.16, 160.20 - 1.4813688 * d],
        3 => [
            317.68143 - 0.1061 * t,
            52.88650 - 0.0609 * t,
            176.630 + 350.89198226 * d,
        ],
        4 => {
            let ja = 99.360714 + 4850.4046 * t;
            let jb = 175.895369 + 1191.9605 * t;
            let jc = 300.323162 + 262.5475 * t;
            let jd = 114.012305 + 6070.2476 * t;
            let je = 49.511251 + 64.3000 * t;
            [
                268.056595 - 0.006499 * t
                    + 0.000117 * sin_deg(ja)
                    + 0.000938 * sin_deg(jb)
                    + 0.001432 * sin_deg(jc)
                    + 0.000030 * sin_deg(jd)
                    + 0.002150 * sin_deg(je),
                64.495303
                    + 0.002413 * t
                    + 0.000050 * cos_deg(ja)
                    + 0.000404 * cos_deg(jb)
                    + 0.000617 * cos_deg(jc)
                    - 0.000013 * cos_deg(jd)
                    + 0.000926 * cos_deg(je),
                284.95 + 870.5360000 * d,
            ]
        }
        5 => [
            40.589 - 0.036 * t,
            83.537 - 0.004 * t,
            38.90 + 810.7939024 * d,
        ],
        6 => [257.311, -15.175, 203.81 - 501.1600928 * d],
        7 => {
            let n = 357.85 + 52.316 * t;
            [
                299.36 + 0.70 * sin_deg(n),
                43.46 - 0.51 * cos_deg(n),
                253.18 + 536.3128492 * d - 0.48 * sin_deg(n),
            ]
        }
        8 => [132.993, -6.163, 302.695 + 56.3625225 * d],

        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "unknown planet index",
            ));
        }
    };
    Ok([[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]].map(|v| {
        let v = rotate_x(rotate_z(v, meridian), 90. - dec);
        rotate_x(rotate_z(v, 90. + ra), -J2000_OBLIQUITY_DEG)
    }))
}
