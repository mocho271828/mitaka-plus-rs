/*
Derived from Mitaka 1.3.1 common/astro/cf96.cpp and Mitaka/mtk_trajectory.cpp.
Copyright (c) 2007 Tsunehiko Kato
Copyright (c) 2014, 2015 Tsunehiko Kato and 4D2U Project,
National Astronomical Observatory of Japan

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
//! CF96, heliocentric J2000 equatorial position (AU) and velocity (AU/day).
//! Times are the ephemeris' Julian days, with no conversion between time scales.

use crate::binary::{Reader, at_path, read_bounded};
use std::{io, path::Path};

const BEGIN: f64 = 2_415_020.5;
const END: f64 = 2_487_980.5;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EphemerisState {
    pub position_au: [f64; 3],
    pub velocity_au_per_day: [f64; 3],
}

#[derive(Debug)]
struct Series {
    secular: Vec<f64>,
    harmonics: Vec<(Vec<f64>, Vec<f64>)>,
}
#[derive(Debug)]
struct Planet {
    start: f64,
    step: f64,
    frequency: Vec<Vec<f64>>,
    blocks: Vec<[Series; 3]>,
}
#[derive(Debug)]
pub struct Cf96Ephemeris {
    planets: Vec<Planet>,
}

fn count(r: &mut Reader<'_>, minimum_bytes: usize) -> io::Result<usize> {
    let n = usize::try_from(r.i32()?).map_err(|_| r.invalid("negative count"))?;
    if minimum_bytes != 0 && n > r.remaining() / minimum_bytes {
        return Err(r.invalid("count exceeds remaining input"));
    }
    Ok(n)
}
fn real(r: &mut Reader<'_>) -> io::Result<f64> {
    let v = f64::from_le_bytes(r.array()?);
    if v.is_finite() {
        Ok(v)
    } else {
        Err(r.invalid("non-finite coefficient or time"))
    }
}
fn reals(r: &mut Reader<'_>, n: usize) -> io::Result<Vec<f64>> {
    if n > r.remaining() / 8 {
        return Err(r.invalid("truncated coefficients"));
    }
    (0..n).map(|_| real(r)).collect()
}
fn series(r: &mut Reader<'_>, imax: usize, counts: &[usize]) -> io::Result<Series> {
    let n = count(r, 8)?;
    if n != 2 * imax {
        return Err(r.invalid("inconsistent secular dimension"));
    }
    let secular = reals(r, n)?;
    if count(r, 0)? != counts.len() {
        return Err(r.invalid("inconsistent Poisson dimension"));
    }
    let harmonics = counts
        .iter()
        .map(|&n| {
            if count(r, 0)? != n {
                return Err(r.invalid("inconsistent frequency count"));
            }
            Ok((reals(r, n)?, reals(r, n)?))
        })
        .collect::<io::Result<_>>()?;
    Ok(Series { secular, harmonics })
}
fn planet(r: &mut Reader<'_>) -> io::Result<Planet> {
    let start = real(r)?;
    let step = real(r)?;
    if step <= 0.0 {
        return Err(r.invalid("block duration must be positive"));
    }
    let order = count(r, 0)?;
    let imax = count(r, 0)?;
    let block_count = count(r, 28)?;
    if imax == 0 || block_count == 0 || order + 1 > r.remaining() / 4 || imax > r.remaining() / 16 {
        return Err(r.invalid("invalid CF96 dimensions"));
    }
    let end = start + step * block_count as f64;
    if !end.is_finite() || end < BEGIN || start > END {
        return Err(r.invalid("invalid CF96 coverage"));
    }
    let counts = (0..=order)
        .map(|_| count(r, 8))
        .collect::<io::Result<Vec<_>>>()?;
    let frequency = counts
        .iter()
        .map(|&n| reals(r, n))
        .collect::<io::Result<_>>()?;
    let blocks = (0..block_count)
        .map(|_| {
            Ok([
                series(r, imax, &counts)?,
                series(r, imax, &counts)?,
                series(r, imax, &counts)?,
            ])
        })
        .collect::<io::Result<_>>()?;
    Ok(Planet {
        start,
        step,
        frequency,
        blocks,
    })
}
impl Cf96Ephemeris {
    pub fn load(path: &Path) -> io::Result<Self> {
        at_path(
            path,
            read_bounded(path, 256 * 1024 * 1024).and_then(|b| Self::from_bytes(&b)),
        )
    }
    pub fn from_bytes(bytes: &[u8]) -> io::Result<Self> {
        let mut r = Reader::new(bytes);
        if r.take(4)? != b"CF96" {
            return Err(r.invalid("invalid CF96 signature"));
        }
        let n = count(&mut r, 28)?;
        if n == 0 {
            return Err(r.invalid("empty CF96 ephemeris"));
        }
        let planets = (0..n).map(|_| planet(&mut r)).collect::<io::Result<_>>()?;
        if r.remaining() != 0 {
            return Err(r.invalid("unexpected trailing CF96 bytes"));
        }
        Ok(Self { planets })
    }
    pub fn planet_count(&self) -> usize {
        self.planets.len()
    }
    fn planet(&self, index: usize) -> io::Result<&Planet> {
        self.planets
            .get(index)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "unknown planet index"))
    }
    pub fn time_range(&self, index: usize) -> io::Result<[f64; 2]> {
        let p = self.planet(index)?;
        Ok([
            BEGIN.max(p.start),
            END.min(p.start + p.step * p.blocks.len() as f64),
        ])
    }
    pub fn state(&self, index: usize, jd: f64) -> io::Result<EphemerisState> {
        let p = self.planet(index)?;
        let [start, end] = self.time_range(index)?;
        if !(start..=end).contains(&jd) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "JD outside CF96 coverage",
            ));
        }
        let index = (((jd - p.start) / p.step) as usize).min(p.blocks.len() - 1);
        let x = 2. * (jd - (p.start + index as f64 * p.step)) / p.step - 1.;
        let fx = 0.5 * x * p.step;
        let wt = 2. / p.step;
        let mut result = EphemerisState {
            position_au: [0.; 3],
            velocity_au_per_day: [0.; 3],
        };
        for (axis, series) in p.blocks[index].iter().enumerate() {
            let (mut position, mut velocity) = (0., 0.);
            let (mut power, mut previous) = (1., 0.);
            for (i, &coef) in series.secular.iter().enumerate() {
                position += coef * power;
                if i > 0 {
                    velocity += i as f64 * wt * coef * previous;
                }
                previous = power;
                power *= x;
            }
            (power, previous) = (1., 0.);
            for (m, (frequencies, (cosine, sine))) in
                p.frequency.iter().zip(&series.harmonics).enumerate()
            {
                for ((&freq, &a), &b) in frequencies.iter().zip(cosine).zip(sine) {
                    let (s, c) = (freq * fx).sin_cos();
                    let term = a * c + b * s;
                    position += term * power;
                    velocity += freq * (b * c - a * s) * power;
                    if m > 0 {
                        velocity += m as f64 * wt * term * previous;
                    }
                }
                previous = power;
                power *= x;
            }
            result.position_au[axis] = position * 1e-10;
            result.velocity_au_per_day[axis] = velocity * 1e-10;
            if !result.position_au[axis].is_finite()
                || !result.velocity_au_per_day[axis].is_finite()
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "non-finite ephemeris result",
                ));
            }
        }
        Ok(result)
    }
}
