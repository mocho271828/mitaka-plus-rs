/*
Copyright (c) 2007-2015 Tsunehiko Kato and 4D2U Project,
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

NGC/SDSSのレコード定義は公式Mitaka 1.3.1を参照。由来は docs/core/EXTENDED_CATALOG_PROVENANCE.md を参照。
*/
//! NGC/IC and SDSS binary formats and HIP name/exoplanet text tables. Positions
//! are J2000 equatorial degrees. Localized names remain in their input encoding.
use crate::{
    binary::{Reader, at_path, read_bounded},
    catalog::CatalogLimits,
};
use std::{collections::HashSet, io, path::Path};
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SdssKind {
    Galaxy,
    Quasar,
}
#[derive(Debug, Clone, Copy)]
pub struct SdssRecord {
    pub ra_deg: f32,
    pub dec_deg: f32,
    pub distance_pc: f32,
    pub classification_value: f32,
}
#[derive(Debug, Clone, Copy)]
pub struct NgcRecord {
    pub number: i16,
    pub is_ngc: bool,
    pub ra_deg: f32,
    pub dec_deg: f32,
    pub kind: u8,
    pub size_arcmin: f32,
    pub magnitude: f32,
    pub photographic_magnitude: bool,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StarName {
    pub hip: i32,
    pub featured: bool,
    pub name_bytes: Vec<u8>,
    pub localized_name_bytes: Vec<u8>,
    pub source_row: usize,
}
fn coordinates(r: &mut Reader<'_>) -> io::Result<[f32; 2]> {
    let (ra, dec) = (r.f32()?, r.f32()?);
    if !(0.0..360.).contains(&ra) || !(-90.0..=90.).contains(&dec) {
        return Err(r.invalid("invalid J2000 coordinates"));
    }
    Ok([ra, dec])
}
fn flag(r: &mut Reader<'_>) -> io::Result<bool> {
    match r.u8()? {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(r.invalid("flag must be zero or one")),
    }
}
fn fixed<T>(
    bytes: &[u8],
    width: usize,
    l: CatalogLimits,
    mut decode: impl FnMut(&mut Reader<'_>) -> io::Result<T>,
) -> io::Result<Vec<T>> {
    let mut r = Reader::new(bytes);
    if bytes.len() as u64 > l.max_file_bytes
        || !bytes.len().is_multiple_of(width)
        || bytes.len() / width > l.max_records
    {
        return Err(r.invalid("invalid fixed record length or catalog limit"));
    }
    (0..bytes.len() / width).map(|_| decode(&mut r)).collect()
}
pub fn parse_ngc(bytes: &[u8], limits: CatalogLimits) -> io::Result<Vec<NgcRecord>> {
    let mut ids = HashSet::new();
    fixed(bytes, 21, limits, |r| {
        let number = r.i16()?;
        let is_ngc = flag(r)?;
        let [ra_deg, dec_deg] = coordinates(r)?;
        let kind = r.u8()?;
        let size_arcmin = r.f32()?;
        let magnitude = r.f32()?;
        let photographic_magnitude = flag(r)?;
        if number <= 0 || kind > 11 || size_arcmin < 0. {
            return Err(r.invalid("invalid NGC identifier, class or angular size"));
        }
        if !ids.insert((is_ngc, number)) {
            return Err(r.invalid("duplicate NGC/IC identifier"));
        }
        Ok(NgcRecord {
            number,
            is_ngc,
            ra_deg,
            dec_deg,
            kind,
            size_arcmin,
            magnitude,
            photographic_magnitude,
        })
    })
}
pub fn parse_sdss(
    bytes: &[u8],
    _kind: SdssKind,
    limits: CatalogLimits,
) -> io::Result<Vec<SdssRecord>> {
    fixed(bytes, 16, limits, |r| {
        let [ra_deg, dec_deg] = coordinates(r)?;
        let distance_pc = r.f32()?;
        let classification_value = r.f32()?;
        if distance_pc < 0. {
            return Err(r.invalid("negative SDSS distance"));
        }
        Ok(SdssRecord {
            ra_deg,
            dec_deg,
            distance_pc,
            classification_value,
        })
    })
}
pub fn load_ngc(path: &Path) -> io::Result<Vec<NgcRecord>> {
    let l = CatalogLimits::default();
    at_path(
        path,
        read_bounded(path, l.max_file_bytes).and_then(|b| parse_ngc(&b, l)),
    )
}
pub fn load_sdss(path: &Path, kind: SdssKind) -> io::Result<Vec<SdssRecord>> {
    let l = CatalogLimits::default();
    at_path(
        path,
        read_bounded(path, l.max_file_bytes).and_then(|b| parse_sdss(&b, kind, l)),
    )
}
fn trim(b: &[u8]) -> &[u8] {
    fn space(c: &u8) -> bool {
        matches!(c, b' ' | b'\t' | b'\r')
    }
    let start = b.iter().position(|c| !space(c)).unwrap_or(b.len());
    let end = b.iter().rposition(|c| !space(c)).map_or(start, |i| i + 1);
    &b[start..end]
}
fn hip(b: &[u8]) -> Option<i32> {
    let b = trim(b);
    if b.is_empty() || !b.iter().all(u8::is_ascii_digit) {
        return None;
    }
    std::str::from_utf8(b)
        .ok()?
        .parse::<i32>()
        .ok()
        .filter(|&v| v > 0)
}
fn text_rows<T>(
    b: &[u8],
    l: CatalogLimits,
    mut parse: impl FnMut(usize, &[u8]) -> Result<T, &'static str>,
) -> io::Result<Vec<T>> {
    if b.len() as u64 > l.max_file_bytes {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "text exceeds catalog byte limit",
        ));
    }
    let mut rows = vec![];
    let mut offset = 0;
    for (i, raw) in b.split_inclusive(|&b| b == b'\n').enumerate() {
        let err = |reason: &str| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("byte {offset}, line {}: {reason}", i + 1),
            )
        };
        let raw = raw.strip_suffix(b"\n").unwrap_or(raw);
        if raw.len() > 4096 || raw.contains(&0) {
            return Err(err("text contains a NUL or line exceeds 4096 bytes"));
        }
        let line = raw.strip_suffix(b"\r").unwrap_or(raw);
        if !matches!(trim(line).first(), None | Some(b'%' | b'#')) {
            if rows.len() == l.max_records {
                return Err(err("catalog exceeds record limit"));
            }
            rows.push(parse(i + 1, line).map_err(err)?);
        }
        offset += raw.len() + 1;
    }
    Ok(rows)
}
pub fn parse_star_names(b: &[u8], l: CatalogLimits) -> io::Result<Vec<StarName>> {
    let mut hips = HashSet::new();
    text_rows(b, l, |source_row, line| {
        if line.len() < 8 || !matches!(line[6], b'*' | b' ') {
            return Err("invalid star-name columns");
        }
        let hip = hip(&line[..6]).ok_or("invalid HIP identifier")?;
        if !hips.insert(hip) {
            return Err("duplicate star-name HIP identifier");
        }
        let names = &line[7..];
        let boundary = names
            .windows(2)
            .position(|p| p == b"  ")
            .unwrap_or(names.len());
        let name = trim(&names[..boundary]);
        if name.is_empty() {
            return Err("empty star name");
        }
        Ok(StarName {
            hip,
            featured: line[6] == b'*',
            name_bytes: name.to_vec(),
            localized_name_bytes: trim(&names[boundary..]).to_vec(),
            source_row,
        })
    })
}
pub fn parse_exoplanet_hosts(b: &[u8], l: CatalogLimits) -> io::Result<Vec<i32>> {
    text_rows(b, l, |_, line| hip(line).ok_or("invalid HIP identifier"))
}
pub fn load_star_names(path: &Path) -> io::Result<Vec<StarName>> {
    let l = CatalogLimits::default();
    at_path(
        path,
        read_bounded(path, l.max_file_bytes).and_then(|b| parse_star_names(&b, l)),
    )
}
pub fn load_exoplanet_hosts(path: &Path) -> io::Result<Vec<i32>> {
    let l = CatalogLimits::default();
    at_path(
        path,
        read_bounded(path, l.max_file_bytes).and_then(|b| parse_exoplanet_hosts(&b, l)),
    )
}
