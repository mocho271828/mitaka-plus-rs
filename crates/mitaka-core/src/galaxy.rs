//! Native Plus GC, NBG and Milky Way data formats from Mitaka_plus f9fe624,
//! galaxy_catalog.cpp and our_galaxy_model.cpp. The GC and NBG readers must be
//! selected by the caller; gc.dat and NBG2.dat have different formats.
use crate::{
    astronomy::{Vector3, equatorial_direction, j2000_equatorial_to_ecliptic},
    binary::{Reader, at_path, read_bounded},
    catalog::CatalogLimits,
};
use std::{io, path::Path};

#[derive(Debug)]
pub struct Catalog<T> {
    pub raw: Vec<u8>,
    pub records: Vec<T>,
}
#[derive(Clone, Copy, Debug)]
pub struct GlobularCluster {
    pub name_bytes: [u8; 12],
    pub ra_deg: f32,
    pub dec_deg: f32,
    pub distance_pc: f32,
    pub core_radius_pc: f32,
    pub half_light_radius_pc: f32,
    pub bv: f32,
    pub vr: f32,
}
#[derive(Clone, Copy, Debug)]
pub struct NearbyGalaxy {
    pub ra_deg: f32,
    pub dec_deg: f32,
    pub distance_pc: f32,
    pub diameter_arcmin: f32,
    pub minor_major_ratio: f32,
    pub render_class: u8,
    pub auxiliary_code: u8,
    pub catalog_code: u8,
    pub catalog_number: i32,
}
fn raw_float(r: &mut Reader<'_>) -> io::Result<f32> {
    Ok(f32::from_le_bytes(r.array()?))
}
fn fixed<T>(
    bytes: Vec<u8>,
    limits: CatalogLimits,
    width: usize,
    decode: impl Fn(&mut Reader<'_>) -> io::Result<T>,
) -> io::Result<Catalog<T>> {
    let mut r = Reader::new(&bytes);
    if !bytes.len().is_multiple_of(width)
        || bytes.len() as u64 > limits.max_file_bytes
        || bytes.len() / width > limits.max_records
    {
        return Err(r.invalid("invalid galaxy catalog length or record limit"));
    }
    let records = (0..bytes.len() / width)
        .map(|_| decode(&mut r))
        .collect::<io::Result<_>>()?;
    Ok(Catalog {
        raw: bytes,
        records,
    })
}
pub fn parse_globular_clusters(
    bytes: Vec<u8>,
    limits: CatalogLimits,
) -> io::Result<Catalog<GlobularCluster>> {
    fixed(bytes, limits, 40, |r| {
        Ok(GlobularCluster {
            name_bytes: r.array()?,
            ra_deg: raw_float(r)?,
            dec_deg: raw_float(r)?,
            distance_pc: raw_float(r)?,
            core_radius_pc: raw_float(r)?,
            half_light_radius_pc: raw_float(r)?,
            bv: raw_float(r)?,
            vr: raw_float(r)?,
        })
    })
}
pub fn parse_nearby_galaxies(
    bytes: Vec<u8>,
    limits: CatalogLimits,
) -> io::Result<Catalog<NearbyGalaxy>> {
    fixed(bytes, limits, 27, |r| {
        Ok(NearbyGalaxy {
            ra_deg: raw_float(r)?,
            dec_deg: raw_float(r)?,
            distance_pc: raw_float(r)?,
            diameter_arcmin: raw_float(r)?,
            minor_major_ratio: raw_float(r)?,
            render_class: r.u8()?,
            auxiliary_code: r.u8()?,
            catalog_code: r.u8()?,
            catalog_number: r.i32()?,
        })
    })
}
pub fn load_globular_clusters(path: &Path) -> io::Result<Catalog<GlobularCluster>> {
    let limits = CatalogLimits::default();
    at_path(
        path,
        read_bounded(path, limits.max_file_bytes).and_then(|b| parse_globular_clusters(b, limits)),
    )
}
pub fn load_nearby_galaxies(path: &Path) -> io::Result<Catalog<NearbyGalaxy>> {
    let limits = CatalogLimits::default();
    at_path(
        path,
        read_bounded(path, limits.max_file_bytes).and_then(|b| parse_nearby_galaxies(b, limits)),
    )
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PositionIssue {
    NonfiniteRa,
    RaOutOfRange,
    NonfiniteDec,
    DecOutOfRange,
    NonfiniteDistance,
    NegativeDistance,
    ZeroDistance,
}
#[derive(Debug, Clone, Copy)]
pub struct GalaxyPositionPc {
    pub equatorial: Vector3,
    pub ecliptic: Vector3,
}
#[derive(Debug)]
pub struct GalaxyPositionResult {
    pub position: Option<GalaxyPositionPc>,
    pub issues: Vec<PositionIssue>,
}
pub fn evaluate_position_pc(ra: f32, dec: f32, distance: f32) -> GalaxyPositionResult {
    use PositionIssue::*;
    let mut issues = Vec::new();
    if !ra.is_finite() {
        issues.push(NonfiniteRa);
    } else if !(0.0..360.).contains(&ra) {
        issues.push(RaOutOfRange);
    }
    if !dec.is_finite() {
        issues.push(NonfiniteDec);
    } else if !(-90.0..=90.).contains(&dec) {
        issues.push(DecOutOfRange);
    }
    if !distance.is_finite() {
        issues.push(NonfiniteDistance);
    } else if distance < 0. {
        issues.push(NegativeDistance);
    } else if distance == 0. {
        issues.push(ZeroDistance);
    }
    let position = if issues.iter().all(|&issue| issue == ZeroDistance) {
        let direction =
            equatorial_direction(ra.into(), dec.into()).expect("validated RA and declination");
        let equatorial = direction.map(|v| v * f64::from(distance));
        let ecliptic =
            j2000_equatorial_to_ecliptic(equatorial).expect("finite f32 parsec distance fits f64");
        Some(GalaxyPositionPc {
            equatorial,
            ecliptic,
        })
    } else {
        None
    };
    GalaxyPositionResult { position, issues }
}
#[derive(Debug, PartialEq)]
pub enum NameKind {
    Blank,
    Comment,
    Entry,
    Unresolved(&'static str),
}
#[derive(Debug)]
pub struct GalaxyNameLine {
    pub source_row: usize,
    pub byte_offset: usize,
    pub raw: Vec<u8>,
    pub key: Vec<u8>,
    pub payload: Vec<u8>,
    pub featured: bool,
    pub kind: NameKind,
}
fn trim_start(bytes: &[u8]) -> &[u8] {
    let start = bytes
        .iter()
        .position(|c| !matches!(c, b' ' | b'\t' | b'\r'))
        .unwrap_or(bytes.len());
    &bytes[start..]
}
pub fn parse_names(bytes: Vec<u8>, limits: CatalogLimits) -> io::Result<Catalog<GalaxyNameLine>> {
    let space = |c: &u8| matches!(c, b' ' | b'\t' | b'\r');
    if bytes.len() as u64 > limits.max_file_bytes {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "name table exceeds byte limit",
        ));
    }
    let mut records = Vec::new();
    let mut offset = 0;
    for line in bytes.split_inclusive(|&c| c == b'\n') {
        if records.len() == limits.max_records {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("byte {offset}: name table exceeds row limit"),
            ));
        }
        let raw = line.strip_suffix(b"\n").unwrap_or(line);
        let mut view = trim_start(raw);
        let (mut key, mut payload, mut featured) = (vec![], vec![], false);
        let kind = if view.is_empty() {
            NameKind::Blank
        } else if matches!(view[0], b'%' | b'#') {
            NameKind::Comment
        } else if view.contains(&0) {
            NameKind::Unresolved("NUL byte in name line")
        } else {
            let n = view.iter().position(space).unwrap_or(view.len());
            key.extend(&view[..n]);
            view = trim_start(&view[n..]);
            if let Some(rest) = view.strip_prefix(b"*") {
                featured = true;
                view = rest;
            }
            payload.extend(view);
            if view.iter().all(space) {
                NameKind::Unresolved("missing name payload")
            } else {
                NameKind::Entry
            }
        };
        records.push(GalaxyNameLine {
            source_row: records.len() + 1,
            byte_offset: offset,
            raw: raw.to_vec(),
            key,
            payload,
            featured,
            kind,
        });
        offset += line.len();
    }
    Ok(Catalog {
        raw: bytes,
        records,
    })
}
pub fn load_names(path: &Path) -> io::Result<Catalog<GalaxyNameLine>> {
    let limits = CatalogLimits::default();
    at_path(
        path,
        read_bounded(path, limits.max_file_bytes).and_then(|b| parse_names(b, limits)),
    )
}
#[derive(Debug)]
pub struct GalaxyMap {
    pub width: usize,
    pub height: usize,
    pub values: Vec<f32>,
    pub nonfinite_count: usize,
    pub outside_unit_interval_count: usize,
}
#[derive(Debug)]
pub struct PointCloud {
    pub positions_pc: Vec<[f32; 3]>,
    pub nonfinite_count: usize,
}
#[derive(Debug)]
pub struct OurGalaxyModel {
    pub raw: Vec<u8>,
    pub maps: [GalaxyMap; 2],
    pub clouds: [PointCloud; 4],
}
impl OurGalaxyModel {
    pub fn load(path: &Path) -> io::Result<Self> {
        let limits = CatalogLimits::default();
        at_path(
            path,
            read_bounded(path, limits.max_file_bytes).and_then(|b| Self::from_bytes(b, limits)),
        )
    }
    pub fn from_bytes(bytes: Vec<u8>, limits: CatalogLimits) -> io::Result<Self> {
        let mut r = Reader::new(&bytes);
        if bytes.len() as u64 > limits.max_file_bytes {
            return Err(r.invalid("Milky Way model exceeds byte limit"));
        }
        let mut total_samples = 0;
        let mut map = || -> io::Result<GalaxyMap> {
            let width = usize::try_from(r.i32()?).map_err(|_| r.invalid("negative map width"))?;
            let height = usize::try_from(r.i32()?).map_err(|_| r.invalid("negative map height"))?;
            let count = width
                .checked_mul(height)
                .ok_or_else(|| r.invalid("map dimension overflow"))?;
            if count == 0 || count > limits.max_records - total_samples || count > r.remaining() / 4
            {
                return Err(r.invalid("invalid map dimensions"));
            }
            total_samples += count;
            let values = (0..count)
                .map(|_| raw_float(&mut r))
                .collect::<io::Result<Vec<_>>>()?;
            let nonfinite_count = values.iter().filter(|x| !x.is_finite()).count();
            let outside_unit_interval_count = values
                .iter()
                .filter(|&&x| x.is_finite() && !(0.0..=1.).contains(&x))
                .count();
            Ok(GalaxyMap {
                width,
                height,
                values,
                nonfinite_count,
                outside_unit_interval_count,
            })
        };
        let maps = [map()?, map()?];
        let mut total_points = 0;
        let mut cloud = || -> io::Result<PointCloud> {
            let count = usize::try_from(r.i32()?).map_err(|_| r.invalid("negative point count"))?;
            if count > limits.max_records - total_points || count > r.remaining() / 12 {
                return Err(r.invalid("invalid point count"));
            }
            total_points += count;
            let positions_pc = (0..count)
                .map(|_| Ok([raw_float(&mut r)?, raw_float(&mut r)?, raw_float(&mut r)?]))
                .collect::<io::Result<Vec<_>>>()?;
            let nonfinite_count = positions_pc
                .iter()
                .filter(|p| p.iter().any(|v| !v.is_finite()))
                .count();
            Ok(PointCloud {
                positions_pc,
                nonfinite_count,
            })
        };
        let clouds = [cloud()?, cloud()?, cloud()?, cloud()?];
        if r.remaining() > 0 {
            return Err(r.invalid("unexpected trailing galaxy model bytes"));
        }
        Ok(Self {
            raw: bytes,
            maps,
            clouds,
        })
    }
}
