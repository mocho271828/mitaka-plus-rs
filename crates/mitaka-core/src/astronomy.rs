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
//! Proleptic Gregorian calendar, J2000 equatorial geometry and Mitaka Earth
//! rotation. No atmospheric refraction, time-scale conversion or leap seconds.
use std::io;

pub const J2000: f64 = 2451545.;
pub const J2000_OBLIQUITY_DEG: f64 = 84381.448 / 3600.;
pub type Vector3 = [f64; 3];

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GregorianDateTime {
    pub year: i32,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: f64,
}

fn invalid(reason: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, reason)
}
pub fn julian_date(date: GregorianDateTime) -> io::Result<f64> {
    const LENGTHS: [u8; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    const BEFORE: [u16; 12] = [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];
    let GregorianDateTime {
        year,
        month,
        day,
        hour,
        minute,
        second,
    } = date;
    if !(-4712..=9999).contains(&year) || !(1..=12).contains(&month) {
        return Err(invalid("Gregorian year/month out of range"));
    }
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let index = usize::from(month - 1);
    if day == 0
        || day > LENGTHS[index] + u8::from(month == 2 && leap)
        || hour >= 24
        || minute >= 60
        || !(0.0..60.).contains(&second)
    {
        return Err(invalid("invalid Gregorian date/time"));
    }
    let y = f64::from(year - 1);
    let start = (365.25 * y).floor() - (y / 100.).floor() + (y / 400.).floor() + 1721425.;
    let days = BEFORE[index] + u16::from(leap && month > 2) + u16::from(day);
    Ok(start + f64::from(days) - 0.5
        + (f64::from(hour) * 3600. + f64::from(minute) * 60. + second) / 86400.)
}
pub(crate) fn valid_jd(jd: f64) -> io::Result<()> {
    // Gregorian -4712-01-01 and 10000-01-01 at midnight.
    if (37.5..5373484.5).contains(&jd) {
        Ok(())
    } else {
        Err(invalid("JD outside Gregorian year -4712..9999"))
    }
}
pub fn gregorian_date_time(jd: f64) -> io::Result<GregorianDateTime> {
    valid_jd(jd)?;
    let day_number = (jd + 0.5).floor() as i64;
    let mut l = day_number + 68569;
    let n = 4 * l / 146097;
    l -= (146097 * n + 3) / 4;
    let i = 4000 * (l + 1) / 1461001;
    l = l - 1461 * i / 4 + 31;
    let j = 80 * l / 2447;
    let day = (l - 2447 * j / 80) as u8;
    l = j / 11;
    let month = (j + 2 - 12 * l) as u8;
    let year = (100 * (n - 49) + i + l) as i32;
    let seconds = (jd - (day_number as f64 - 0.5)) * 86400.;
    let hour = (seconds / 3600.) as u8;
    let seconds = seconds - f64::from(hour) * 3600.;
    let minute = (seconds / 60.) as u8;
    Ok(GregorianDateTime {
        year,
        month,
        day,
        hour,
        minute,
        second: seconds - f64::from(minute) * 60.,
    })
}

pub(crate) fn centered(angle: f64, period: f64) -> f64 {
    let rem = angle % period;
    if rem > period / 2. {
        rem - period
    } else if rem < -period / 2. {
        rem + period
    } else {
        rem
    }
}
fn positive(degrees: f64) -> f64 {
    let v = degrees.rem_euclid(360.);
    if v >= 360. || v == 0. { 0. } else { v }
}
pub(crate) fn rotate_x(v: Vector3, angle_deg: f64) -> Vector3 {
    let (s, c) = centered(angle_deg, 360.).to_radians().sin_cos();
    [v[0], c * v[1] - s * v[2], s * v[1] + c * v[2]]
}
pub(crate) fn rotate_z(v: Vector3, angle_deg: f64) -> Vector3 {
    let (s, c) = centered(angle_deg, 360.).to_radians().sin_cos();
    [c * v[0] - s * v[1], s * v[0] + c * v[1], v[2]]
}
fn dot(a: Vector3, b: Vector3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
pub fn equatorial_direction(ra_deg: f64, dec_deg: f64) -> io::Result<Vector3> {
    if !ra_deg.is_finite() || !(-90.0..=90.).contains(&dec_deg) {
        return Err(invalid("invalid equatorial coordinates"));
    }
    let (s, c) = centered(ra_deg, 360.).to_radians().sin_cos();
    let (sd, cd) = dec_deg.to_radians().sin_cos();
    Ok([cd * c, cd * s, sd])
}
pub fn equatorial_coordinates(v: Vector3) -> io::Result<[f64; 2]> {
    if !v.iter().all(|v| v.is_finite()) {
        return Err(invalid("non-finite direction"));
    }
    let scale = v.iter().fold(0f64, |a, v| a.max(v.abs()));
    if scale == 0. {
        return Err(invalid("zero direction vector"));
    }
    let [x, y, z] = v.map(|v| v / scale);
    Ok([
        positive(y.atan2(x).to_degrees()),
        z.atan2(x.hypot(y)).to_degrees(),
    ])
}
fn rotate_obliquity(v: Vector3, angle: f64) -> io::Result<Vector3> {
    let out = rotate_x(v, angle);
    if out.iter().all(|n| n.is_finite()) {
        Ok(out)
    } else {
        Err(invalid("coordinate overflow"))
    }
}
pub fn j2000_ecliptic_to_equatorial(v: Vector3) -> io::Result<Vector3> {
    rotate_obliquity(v, J2000_OBLIQUITY_DEG)
}
pub fn j2000_equatorial_to_ecliptic(v: Vector3) -> io::Result<Vector3> {
    rotate_obliquity(v, -J2000_OBLIQUITY_DEG)
}

/// Earth's fixed-to-equatorial rotation from Mitaka 1.3.1 mtk_planet_orientation.cpp.
/// The epoch tilt used in the precession polynomial differs from the J2000 ecliptic tilt.
pub(crate) fn earth_axes(jd: f64) -> [Vector3; 3] {
    let d = jd - J2000;
    let t = d / 36525.;
    let t2 = t * t;
    let t3 = t * t2;
    let t4 = t2 * t2;
    let t5 = t4 * t;
    let eps0 = 84381.406 / 3600.;
    let phi = (5038.481507 * t - 1.0790069 * t2 - 0.00114045 * t3 + 0.000132851 * t4
        - 0.0000000951 * t5)
        / 3600.;
    let omega = eps0
        + (-0.025754 * t + 0.0512623 * t2 - 0.00772503 * t3 - 0.000000467 * t4 + 0.0000003337 * t5)
            / 3600.;
    let chi = (10.556403 * t - 2.3814292 * t2 - 0.00121197 * t3 + 0.000170663 * t4
        - 0.0000000560 * t5)
        / 3600.;
    let spin = 360. * (0.7790572732640 + 1.002_737_811_911_354_5 * d);
    [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]].map(|v| {
        let v = rotate_z(rotate_z(v, spin), chi);
        let v = rotate_z(rotate_x(v, -omega), -phi);
        rotate_x(v, eps0)
    })
}
#[derive(Debug, Clone, Copy)]
pub struct ObservationFrame {
    east: Vector3,
    north: Vector3,
    up: Vector3,
}
impl ObservationFrame {
    pub fn new(latitude_deg: f64, longitude_deg: f64, jd: f64) -> io::Result<Self> {
        valid_jd(jd)?;
        if !(-90.0..=90.).contains(&latitude_deg) || !(-180.0..=180.).contains(&longitude_deg) {
            return Err(invalid("observer site outside latitude/longitude bounds"));
        }
        let axes = earth_axes(jd);
        let transform = |a: Vector3| {
            std::array::from_fn(|i| a[0] * axes[0][i] + a[1] * axes[1][i] + a[2] * axes[2][i])
        };
        let (sl, cl) = latitude_deg.to_radians().sin_cos();
        let (so, co) = longitude_deg.to_radians().sin_cos();
        Ok(Self {
            east: transform([-so, co, 0.]),
            north: transform([-sl * co, -sl * so, cl]),
            up: transform([cl * co, cl * so, sl]),
        })
    }
    /// Returns [azimuth in degrees east from north, altitude in degrees].
    pub fn to_horizontal(&self, ra_deg: f64, dec_deg: f64) -> io::Result<[f64; 2]> {
        let dir = equatorial_direction(ra_deg, dec_deg)?;
        let (east, north) = (dot(dir, self.east), dot(dir, self.north));
        let horizontal = east.hypot(north);
        let az = if horizontal <= 1e-14 {
            0.
        } else {
            positive(east.atan2(north).to_degrees())
        };
        Ok([az, dot(dir, self.up).atan2(horizontal).to_degrees()])
    }
    pub fn to_equatorial(&self, az_deg: f64, alt_deg: f64) -> io::Result<[f64; 2]> {
        if !az_deg.is_finite() || !(-90.0..=90.).contains(&alt_deg) {
            return Err(invalid("invalid horizontal coordinates"));
        }
        let (sa, ca) = centered(az_deg, 360.).to_radians().sin_cos();
        let (up, h) = alt_deg.to_radians().sin_cos();
        equatorial_coordinates(std::array::from_fn(|i| {
            h * sa * self.east[i] + h * ca * self.north[i] + up * self.up[i]
        }))
    }
}

pub fn greenwich_mean_sidereal_time_deg(jd: f64) -> io::Result<f64> {
    valid_jd(jd)?;
    let midnight = (jd - 0.5).floor() + 0.5;
    let centuries = (jd - J2000) / 36525.;
    let hours = (jd - midnight) * 24.;
    Ok(positive(
        15. * (6.697375
            + 0.065709824279 * (midnight - J2000)
            + 1.0027379 * hours
            + 0.0000258 * centuries * centuries),
    ))
}
pub fn local_mean_sidereal_time_deg(jd: f64, east_longitude_deg: f64) -> io::Result<f64> {
    if !(-180.0..=180.).contains(&east_longitude_deg) {
        return Err(invalid("invalid observer longitude"));
    }
    Ok(positive(
        greenwich_mean_sidereal_time_deg(jd)? + east_longitude_deg,
    ))
}
