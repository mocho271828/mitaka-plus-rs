//! Builds one empty registration container per native system source. CF96 binding,
//! structural parents and named pair corrections follow Mitaka_plus f9fe624,
//! satellite_{registration,model,absolute}.cpp and satellite_browser_model.mm.
mod orbit;
use crate::{
    astronomy::{Vector3, j2000_equatorial_to_ecliptic},
    ephemeris::Cf96Ephemeris,
    lunar::lunar_position,
    satellite::{RotationInput, legacy_parent_pole, relative_position_km, satellite_rotation},
    satellite_catalog::{Dataset, SourceKind},
};
use orbit::Ellipse;
use std::{collections::HashMap, io, sync::Arc};
pub const KM_PER_AU: f64 = 149_597_870.7;
const EARTH_FRACTION: f64 = 0.012150584133256915;
const PLUTO_FRACTION: f64 = 0.07407407407407407;
const PLANETS: [&[u8]; 9] = [
    b"Mercury", b"Venus", b"Earth", b"Mars", b"Jupiter", b"Saturn", b"Uranus", b"Neptune", b"Pluto",
];
fn invalid(reason: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, reason)
}
#[derive(Debug)]
struct Satellite {
    ordinal: usize,
    parent: usize,
    rotation: RotationInput,
}
#[derive(Debug)]
pub struct Model {
    pub source_index: usize,
    pub source_key: String,
    planet_ordinals: Vec<usize>,
    satellites: Vec<Satellite>,
    record_count: usize,
    names_complete: bool,
}
#[derive(Debug)]
pub struct Snapshot {
    pub jd: f64,
    positions: Vec<Result<Vector3, String>>,
    orbits: HashMap<usize, Ellipse>,
    pub pair_corrected_ordinals: Vec<usize>,
}
impl Snapshot {
    /// A closed 256-segment instantaneous orbit in heliocentric J2000 ecliptic AU.
    /// Ordinals are local to this source. The analytic Moon has no Kepler ellipse.
    pub fn orbit_au(&self, ordinal: usize) -> io::Result<Arc<[Vector3]>> {
        let position = self.position_au(ordinal)?;
        let orbit = self
            .orbits
            .get(&ordinal)
            .ok_or_else(|| invalid("no instantaneous Kepler orbit for this definition"))?;
        // A catalog can have many more entries than either of the two standard source files.
        // Bound the point cache; large custom systems can still request every orbit.
        orbit.path(self.jd, position, self.orbits.len() <= 1024)
    }

    /// Input is the ordinal in the original system file, including system and sun blocks.
    pub fn position_au(&self, ordinal: usize) -> io::Result<Vector3> {
        self.positions
            .get(ordinal)
            .ok_or_else(|| invalid("unknown definition ordinal"))?
            .as_ref()
            .copied()
            .map_err(|e| invalid(e))
    }
}
impl Model {
    pub fn from_source(data: &Dataset, source_index: usize) -> io::Result<Self> {
        let source = data
            .sources
            .get(source_index)
            .filter(|s| s.kind == SourceKind::System && s.read_complete && s.parsed)
            .ok_or_else(|| invalid("requires a fully read system source"))?;
        let mut planet_ordinals = Vec::new();
        let mut satellites = Vec::new();
        let mut names_complete = true;
        let mut charon = None;
        for (ordinal, r) in source.records.iter().enumerate() {
            let parent = r.parent.and_then(|i| source.records.get(i));
            let structural = match r.block.as_str() {
                "Ssystem" => r.parent.is_none(),
                "Sun" => parent.is_some_and(|p| p.block == "Ssystem"),
                "Planet" => parent.is_some_and(|p| p.block == "Sun"),
                "Satellite" => parent.is_some_and(|p| p.block == "Planet"),
                _ => false,
            };
            if !structural || r.ordinal != ordinal {
                return Err(invalid("unsupported system hierarchy or ordinal"));
            }
            if !matches!(r.block.as_str(), "Planet" | "Satellite") {
                continue;
            }
            let orbit = r
                .orbit_source
                .and_then(|i| data.sources.get(i))
                .filter(|s| s.kind == SourceKind::Orbit && s.read_complete)
                .ok_or_else(|| invalid("incomplete orbit reference prevents registration"))?;
            let name = r.name().unwrap_or_default();
            if r.block == "Planet" {
                if PLANETS.get(planet_ordinals.len()).copied() != Some(name) {
                    return Err(invalid("planet registration order differs from CF96"));
                }
                planet_ordinals.push(ordinal);
                continue;
            }
            names_complete &= !name.is_empty();
            if name == b"Charon" && charon.is_none() {
                charon = Some(satellites.len());
            }
            let parent = planet_ordinals
                .iter()
                .position(|&i| Some(i) == r.parent)
                .ok_or_else(|| invalid("unregistered structural parent"))?;
            let orbit = orbit.records.first();
            let plane = orbit
                .and_then(|r| r.field("RefPlane"))
                .and_then(|f| match f.value_bytes.as_slice() {
                    [p] => Some(*p),
                    _ => None,
                })
                .unwrap_or(0);
            satellites.push(Satellite {
                ordinal,
                parent,
                rotation: RotationInput {
                    name_bytes: name.to_vec(),
                    reference_plane: plane,
                    elements: orbit.and_then(|r| r.elements()),
                    correct_mars_node: false,
                    charon_index: None,
                    parent_pole: None,
                },
            });
        }
        if planet_ordinals.len() != PLANETS.len() {
            return Err(invalid("CF96 binding requires all nine planets"));
        }
        for s in &mut satellites {
            if s.rotation.reference_plane == b'C' && names_complete {
                s.rotation.charon_index = charon;
            }
        }
        Ok(Self {
            source_index,
            source_key: source.key.clone(),
            planet_ordinals,
            satellites,
            record_count: source.records.len(),
            names_complete,
        })
    }
    pub fn evaluate(&self, ephemeris: &Cf96Ephemeris, jd: f64) -> io::Result<Snapshot> {
        if !(2415020.5..=2487980.5).contains(&jd) {
            return Err(invalid("outside verified satellite CF96 branch"));
        }
        let mut snap = Snapshot {
            jd,
            positions: vec![
                Err("no registered position for this definition".into());
                self.record_count
            ],
            orbits: HashMap::new(),
            pair_corrected_ordinals: Vec::new(),
        };
        for (i, &ordinal) in self.planet_ordinals.iter().enumerate() {
            snap.positions[ordinal] = ephemeris
                .state(i, jd)
                .and_then(|state| j2000_equatorial_to_ecliptic(state.position_au))
                .map_err(|e| e.to_string());
        }
        let rotations: Vec<_> = self
            .satellites
            .iter()
            .map(|s| {
                let mut input = s.rotation.clone();
                if input.reference_plane == b'Q' {
                    input.parent_pole = legacy_parent_pole(s.parent, jd).ok();
                }
                input
            })
            .collect();
        let moon = lunar_position(jd).and_then(j2000_equatorial_to_ecliptic);
        let mut moon_ordinal = None;
        let mut charon_ordinal = None;
        for (idx, s) in self.satellites.iter().enumerate() {
            let name = &s.rotation.name_bytes;
            if name == b"Moon" && moon_ordinal.is_none() {
                moon_ordinal = Some(s.ordinal);
            }
            if name == b"Charon" && charon_ordinal.is_none() {
                charon_ordinal = Some(s.ordinal);
            }
            let result = (|| -> io::Result<Vector3> {
                if name.is_empty() {
                    return Err(invalid("unresolved satellite identifier"));
                }
                let (parent, relative_au, ellipse) = if name == b"Moon" {
                    if !self.names_complete {
                        return Err(invalid("incomplete identifiers in Moon lookup"));
                    }
                    (
                        2,
                        moon.as_ref()
                            .copied()
                            .map_err(|e| invalid(&e.to_string()))?,
                        None,
                    )
                } else {
                    let elements = rotations[idx]
                        .elements
                        .ok_or_else(|| invalid("unresolved satellite elements"))?;
                    let rotation = satellite_rotation(&rotations, idx, jd)?;
                    let relative = relative_position_km(elements, rotation, jd)?;
                    (
                        s.parent,
                        relative.map(|v| v / KM_PER_AU),
                        Some((relative, rotation, elements)),
                    )
                };
                let origin = snap.position_au(self.planet_ordinals[parent])?;
                let p: Vector3 = std::array::from_fn(|axis| origin[axis] + relative_au[axis]);
                if p.iter().any(|v| !v.is_finite()) {
                    return Err(invalid("absolute satellite position overflow"));
                }
                if let Some((relative, rotation, elements)) = ellipse {
                    snap.orbits.insert(
                        s.ordinal,
                        Ellipse::new(origin, relative, rotation, elements),
                    );
                }
                Ok(p)
            })();
            snap.positions[s.ordinal] = result.map_err(|e| e.to_string());
        }
        for (parent, satellite, fraction) in [
            (2, moon_ordinal, EARTH_FRACTION),
            (8, charon_ordinal, PLUTO_FRACTION),
        ] {
            let parent = self.planet_ordinals[parent];
            if !self.names_complete {
                snap.positions[parent] = Err("incomplete special pair lookup".into());
                if let Some(s) = satellite {
                    snap.positions[s] = Err("incomplete special pair lookup".into());
                }
                continue;
            }
            if let Some(satellite) = satellite {
                let corrected = snap.position_au(parent).and_then(|p| {
                    let s = snap.position_au(satellite)?;
                    let delta: Vector3 = std::array::from_fn(|i| (p[i] - s[i]) * fraction);
                    let a: Vector3 = std::array::from_fn(|i| p[i] + delta[i]);
                    let b: Vector3 = std::array::from_fn(|i| s[i] + delta[i]);
                    if a.iter().chain(&b).any(|v| !v.is_finite()) {
                        return Err(invalid("pair correction overflow"));
                    }
                    Ok((a, b))
                });
                match corrected {
                    Ok((a, b)) => {
                        snap.positions[parent] = Ok(a);
                        snap.positions[satellite] = Ok(b);
                        if let Some(ellipse) = snap.orbits.get_mut(&satellite) {
                            ellipse.factor = 1. - fraction;
                        }
                        snap.pair_corrected_ordinals.extend([parent, satellite]);
                    }
                    Err(e) => {
                        snap.positions[parent] = Err(e.to_string());
                        snap.positions[satellite] = Err(e.to_string());
                    }
                }
            }
        }
        Ok(snap)
    }
}
