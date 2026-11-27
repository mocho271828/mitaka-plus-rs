//! Source-preserving small-body orbits from Mitaka_plus f9fe624,
//! small_body_{catalog,text,orbit}.cpp and minor_planet_position.cpp.
mod parser;

use crate::{
    astronomy::{Vector3, centered, rotate_x, rotate_z},
    binary::read_bounded,
    catalog::CatalogLimits,
    orbit::{OrbitalElements, OrbitalState, orbital_state},
};
use sha2::{Digest, Sha256};
use std::{fs, io, ops::Range, path::Path};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Asteroid,
    TransNeptunian,
    Comet,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Encoding {
    Packed36,
    Packed37,
    FixedComet,
    ElementText,
    Aliases,
}

#[derive(Clone, Copy, Debug)]
pub struct Perihelion {
    pub distance_au: f64,
    pub eccentricity: f64,
    pub jd: f64,
    pub ascending_node_deg: f64,
    pub inclination_deg: f64,
    pub argument_deg: f64,
}
#[derive(Clone, Copy, Debug)]
pub enum Orbit {
    Mean(OrbitalElements),
    Perihelion(Perihelion),
    Unresolved,
}

#[derive(Debug)]
pub struct Record {
    pub kind: Kind,
    pub encoding: Encoding,
    pub byte_range: Range<usize>,
    pub number: Option<i32>,
    pub raw_flag: Option<u8>,
    pub epoch_value: Option<f64>,
    pub perihelion_time_value: Option<f64>,
    pub name_bytes: Vec<u8>,
    pub display_name_bytes: Vec<u8>,
    pub aliases: Vec<Vec<u8>>,
    pub fields: Vec<(Vec<u8>, Vec<u8>)>,
    pub orbit: Orbit,
}
impl Record {
    fn new(kind: Kind, encoding: Encoding, byte_range: Range<usize>) -> Self {
        Self {
            kind,
            encoding,
            byte_range,
            number: None,
            raw_flag: None,
            epoch_value: None,
            perihelion_time_value: None,
            name_bytes: Vec::new(),
            display_name_bytes: Vec::new(),
            aliases: Vec::new(),
            fields: Vec::new(),
            orbit: Orbit::Unresolved,
        }
    }

    /// Two-body heliocentric J2000 ecliptic AU. Minor planets take the legacy
    /// numerical JD unchanged. Fixed-width comet perihelia use the JPL TDB convention.
    /// None of the source files declare an accuracy interval.
    pub fn position(&self, jd: f64) -> io::Result<Vector3> {
        if !jd.is_finite() {
            return Err(invalid("non-finite time"));
        }
        match (self.encoding, self.orbit) {
            (Encoding::FixedComet, Orbit::Perihelion(p)) => Ok(p.state(jd)?.position_au),
            (encoding @ (Encoding::Packed36 | Encoding::ElementText), Orbit::Mean(mut e))
                if self.kind != Kind::Comet && self.raw_flag.is_none() =>
            {
                if e.eccentricity == 1. || (encoding == Encoding::Packed36 && e.eccentricity > 1.) {
                    return Err(invalid("unsupported minor-planet eccentricity"));
                }
                if e.eccentricity < 1. && e.mean_anomaly_deg.abs() >= 360. {
                    e.mean_anomaly_deg = centered(e.mean_anomaly_deg, 360.);
                }
                Ok(orbital_state(e, jd)?.position_au)
            }
            _ => Err(invalid("unresolved position contract")),
        }
    }
}
impl Perihelion {
    pub fn state(&self, jd: f64) -> io::Result<OrbitalState> {
        const K: f64 = 0.01720209895;
        let q = self.distance_au;
        let e = self.eccentricity;
        if [
            jd,
            self.jd,
            q,
            e,
            self.ascending_node_deg,
            self.inclination_deg,
            self.argument_deg,
        ]
        .iter()
        .any(|v| !v.is_finite())
            || q <= 0.
            || e < 0.
        {
            return Err(invalid("invalid perihelion elements"));
        }
        if e != 1. {
            let a = q / (1. - e).abs();
            return orbital_state(
                OrbitalElements {
                    epoch_jd: self.jd,
                    semi_major_axis_au: a,
                    eccentricity: e,
                    mean_anomaly_deg: 0.,
                    mean_motion_deg_per_day: K.to_degrees() / a / a.sqrt(),
                    ascending_node_deg: self.ascending_node_deg,
                    inclination_deg: self.inclination_deg,
                    argument_of_periapsis_deg: self.argument_deg,
                },
                jd,
            );
        }
        let b = (3. * K / (2. * std::f64::consts::SQRT_2)) * ((jd - self.jd) / q) / q.sqrt();
        let d = 2. * (b.asinh() / 3.).sinh();
        let scale = std::f64::consts::SQRT_2 * K / q.sqrt() / (1. + d * d);
        let rotate = |v| {
            rotate_z(
                rotate_x(rotate_z(v, self.argument_deg), self.inclination_deg),
                self.ascending_node_deg,
            )
        };
        let position_au = rotate([q * (1. - d * d), 2. * q * d, 0.]);
        let velocity_au_per_day = rotate([-d * scale, scale, 0.]);
        if scale == 0.
            || position_au
                .iter()
                .chain(&velocity_au_per_day)
                .any(|v| !v.is_finite())
        {
            return Err(invalid("parabolic state outside numerical range"));
        }
        Ok(OrbitalState {
            position_au,
            velocity_au_per_day,
        })
    }
}
fn invalid(reason: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, reason)
}

#[derive(Debug)]
pub struct Source {
    pub key: String,
    pub encoding: Encoding,
    pub raw: Vec<u8>,
    pub sha256: Option<[u8; 32]>,
    pub records: Vec<Record>,
    pub error: Option<io::Error>,
}
impl Source {
    pub fn record_bytes(&self, ordinal: usize) -> Option<&[u8]> {
        self.raw.get(self.records.get(ordinal)?.byte_range.clone())
    }
}
#[derive(Debug, Default)]
pub struct Dataset {
    pub sources: Vec<Source>,
    pub directory_errors: Vec<(String, io::Error)>,
}
impl Dataset {
    pub fn load(data_dir: &Path, limits: CatalogLimits) -> Self {
        use Encoding::*;
        use Kind::*;
        let mut data = Self::default();
        let mut remaining = limits.max_records;
        let mut sources = vec![
            ("mp.dat".into(), Asteroid, Packed36),
            ("mp2.dat".into(), Asteroid, Packed37),
            ("TNO.dat".into(), TransNeptunian, Packed36),
            ("TNO2.dat".into(), TransNeptunian, Packed37),
            ("comets.dat".into(), Comet, FixedComet),
        ];
        for (key, kind) in [("orbits/asteroids", Asteroid), ("orbits/comets", Comet)] {
            let list = (|| -> io::Result<Vec<String>> {
                let mut keys = Vec::new();
                for entry in fs::read_dir(data_dir.join(key))? {
                    let path = entry?.path();
                    if path.extension().is_some_and(|v| v == "dat") {
                        let name = path
                            .file_name()
                            .and_then(|v| v.to_str())
                            .ok_or_else(|| invalid("non-UTF8 source key"))?;
                        keys.push(format!("{key}/{name}"));
                    }
                }
                keys.sort();
                Ok(keys)
            })();
            match list {
                Ok(keys) => sources.extend(keys.into_iter().map(|s| (s, kind, ElementText))),
                Err(e) => data.directory_errors.push((key.into(), e)),
            }
        }
        sources.push(("small_objects_name.dat".into(), Asteroid, Aliases));
        for (key, kind, encoding) in sources {
            let mut src = Source {
                key,
                encoding,
                raw: Vec::new(),
                sha256: None,
                records: Vec::new(),
                error: None,
            };
            let result = (|| -> io::Result<()> {
                src.raw = read_bounded(&data_dir.join(&src.key), limits.max_file_bytes)?;
                src.sha256 = Some(Sha256::digest(&src.raw).into());
                if encoding == Aliases {
                    let table = parser::aliases(&src.raw)?;
                    for record in data.sources.iter_mut().flat_map(|s| s.records.iter_mut()) {
                        if let Some(names) = record.number.and_then(|n| table.get(&n)) {
                            record.aliases.clone_from(names);
                        }
                    }
                } else {
                    src.records = parser::records(&src.raw, kind, encoding, remaining)?;
                    remaining -= src.records.len();
                }
                Ok(())
            })();
            src.error = result.err();
            data.sources.push(src);
        }
        data
    }
}
