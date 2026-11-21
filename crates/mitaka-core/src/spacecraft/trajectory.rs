//! Mitaka_plus spacecraft_position.cpp and ephmeris.cpp. The PPI four-point
//! polynomial follows the Mitaka 1.3.1 mtk_trajectory.cpp record layout.
use super::invalid;
use crate::astronomy::Vector3;
use std::io;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sample {
    pub time: f64,
    pub coordinates: Vector3,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sampling {
    Exact,
    Interpolated,
    ClampedBefore,
    ClampedAfter,
}
#[derive(Clone, Copy, Debug)]
pub struct SampledPosition {
    pub position_au: Vector3,
    pub sampling: Sampling,
    pub first: usize,
    pub second: usize,
    pub u: f64,
}
#[derive(Debug)]
pub struct Pth {
    samples: Vec<Sample>,
    pub duplicate_ordinals: Vec<usize>,
}
fn numbers<const N: usize>(bytes: &[u8]) -> io::Result<[f64; N]> {
    let mut out = [0.; N];
    for (field, chunk) in out.iter_mut().zip(bytes.as_chunks::<8>().0) {
        *field = f64::from_le_bytes(*chunk);
        if !field.is_finite() {
            return Err(invalid("non-finite trajectory field"));
        }
    }
    Ok(out)
}
fn count(raw: &[u8], width: usize, limit: usize) -> io::Result<usize> {
    if raw.is_empty() || !raw.len().is_multiple_of(width) || raw.len() / width > limit {
        return Err(invalid("empty, truncated or oversized trajectory"));
    }
    Ok(raw.len() / width)
}
impl Pth {
    pub fn parse(raw: &[u8], max_records: usize) -> io::Result<Self> {
        let mut samples: Vec<Sample> = Vec::with_capacity(count(raw, 32, max_records)?);
        let mut duplicate_ordinals = Vec::new();
        for (idx, chunk) in raw.as_chunks::<32>().0.iter().enumerate() {
            let [time, x, y, z] = numbers(chunk)?;
            if let Some(prev) = samples.last() {
                if prev.time > time {
                    return Err(invalid(&format!("decreasing PTH time at record {idx}")));
                }
                if prev.time == time {
                    duplicate_ordinals.push(idx);
                }
            }
            samples.push(Sample {
                time,
                coordinates: [x, y, z],
            });
        }
        Ok(Self {
            samples,
            duplicate_ordinals,
        })
    }
    pub fn samples(&self) -> &[Sample] {
        &self.samples
    }
    /// Legacy numeric JD, heliocentric J2000 ecliptic AU. The original search
    /// resolves exact duplicates by midpoint, with first/last endpoint precedence.
    pub fn position(&self, jd: f64) -> io::Result<SampledPosition> {
        if !jd.is_finite() {
            return Err(invalid("non-finite PTH time"));
        }
        let last = self.samples.len() - 1;
        let single = |index: usize, sampling| SampledPosition {
            position_au: self.samples[index].coordinates,
            sampling,
            first: index,
            second: index,
            u: 0.,
        };
        if jd <= self.samples[0].time {
            return Ok(single(
                0,
                if jd < self.samples[0].time {
                    Sampling::ClampedBefore
                } else {
                    Sampling::Exact
                },
            ));
        }
        if jd >= self.samples[last].time {
            return Ok(single(
                last,
                if jd > self.samples[last].time {
                    Sampling::ClampedAfter
                } else {
                    Sampling::Exact
                },
            ));
        }
        let (mut first, mut second) = (0, last);
        while second - first > 1 {
            let mid = first + (second - first) / 2;
            let t = self.samples[mid].time;
            if jd == t {
                return Ok(single(mid, Sampling::Exact));
            }
            if t > jd {
                second = mid;
            } else {
                first = mid;
            }
        }
        let a = self.samples[first];
        let b = self.samples[second];
        let span = b.time - a.time;
        let elapsed = jd - a.time;
        let u = elapsed / span;
        if !span.is_finite() || !elapsed.is_finite() || !(0. ..=1.).contains(&u) {
            return Err(invalid("PTH interpolation time outside numerical range"));
        }
        let position_au =
            std::array::from_fn(|i| a.coordinates[i] * (1. - u) + b.coordinates[i] * u);
        if position_au.iter().any(|x| !x.is_finite()) {
            return Err(invalid("PTH coordinate overflow"));
        }
        Ok(SampledPosition {
            position_au,
            sampling: Sampling::Interpolated,
            first,
            second,
            u,
        })
    }
}
#[derive(Clone, Debug)]
pub struct Segment {
    pub times: [f64; 4],
    pub axes: [[f64; 4]; 3],
}
#[derive(Debug)]
pub struct Ppi {
    segments: Vec<Segment>,
}
impl Ppi {
    pub fn parse(raw: &[u8], max_records: usize) -> io::Result<Self> {
        let mut segments: Vec<Segment> = Vec::with_capacity(count(raw, 128, max_records)?);
        for chunk in raw.as_chunks::<128>().0 {
            let times = numbers(&chunk[..32])?;
            if times.windows(2).any(|v| v[0] >= v[1])
                || segments.last().is_some_and(|prev| prev.times[3] > times[0])
            {
                return Err(invalid("unordered PPI times or overlapping segments"));
            }
            let axes = [
                numbers(&chunk[32..64])?,
                numbers(&chunk[64..96])?,
                numbers(&chunk[96..])?,
            ];
            segments.push(Segment { times, axes });
        }
        Ok(Self { segments })
    }
    pub fn segments(&self) -> &[Segment] {
        &self.segments
    }
    /// Native kilometres. The origin, axes and physical time scale are unresolved.
    pub fn native_position_km(&self, jd: f64) -> io::Result<Vector3> {
        if !jd.is_finite() {
            return Err(invalid("non-finite PPI time"));
        }
        let i = self.segments.partition_point(|s| s.times[3] < jd);
        let s = self
            .segments
            .get(i)
            .filter(|s| jd >= s.times[0])
            .ok_or_else(|| invalid("outside PPI coverage"))?;
        let mut position = [0.; 3];
        for i in 0..4 {
            let mut weight = 1.;
            for j in 0..4 {
                if i != j {
                    weight *= (jd - s.times[j]) / (s.times[i] - s.times[j]);
                }
            }
            for (target, axis) in position.iter_mut().zip(&s.axes) {
                *target += weight * axis[i];
            }
        }
        if position.iter().any(|v| !v.is_finite()) {
            return Err(invalid("PPI interpolation overflow"));
        }
        Ok(position)
    }
}
