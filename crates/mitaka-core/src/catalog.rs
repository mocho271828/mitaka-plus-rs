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

Mitaka 1.3.1 mtk_stars.cpp / mtk_constellation.cpp の形式定義から分離。
2026: 固定幅・little-endian 読込、検証、RAII、描画非依存の生データAPIへ変更。
*/
//! HIP and constellation formats from Mitaka 1.3.1, via Mitaka_plus f9fe624.
//! Row numbers are one-based on disk. Names keep their original encoding.

use crate::binary::{Reader, at_path, read_bounded};
use std::{io, path::Path};

pub const HIP_RECORD_BYTES: usize = 45;
#[derive(Clone, Copy, Debug)]
pub struct CatalogLimits {
    pub max_file_bytes: u64,
    pub max_records: usize,
}
impl Default for CatalogLimits {
    fn default() -> Self {
        Self {
            max_file_bytes: 256 * 1024 * 1024,
            max_records: 1_000_000,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Star {
    pub source_row: usize,
    pub raw: [u8; HIP_RECORD_BYTES],
    pub hip: i32,
    pub ra_deg: f32,
    pub dec_deg: f32,
    pub parallax_mas: f32,
    pub parallax_error_mas: f32,
    pub proper_motion_ra_mas_per_year: f32,
    pub proper_motion_dec_mas_per_year: f32,
    pub magnitude: f32,
    pub bv: f32,
    pub spectral_type: i8,
    pub spectral_subtype: i8,
    pub is_double: bool,
    pub flamsteed_number: i16,
    pub bayer_id: i8,
    pub bayer_subscript: i8,
    pub constellation_id: i8,
    pub ads_component: i8,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Constellation {
    pub source_row: usize,
    pub raw: Vec<u8>,
    pub abbreviation: Vec<u8>,
    pub boundary_abbreviation: Vec<u8>,
    pub english_name: Vec<u8>,
    pub english_genitive: Vec<u8>,
    pub japanese_hiragana_bytes: Vec<u8>,
    pub japanese_kanji_bytes: Vec<u8>,
    pub ra_deg: f32,
    pub dec_deg: f32,
    pub hip_lines: Vec<Vec<i32>>,
    /// Boundary points use right ascension in hours and declination in degrees.
    pub boundary: Vec<[f32; 2]>,
}

pub fn load_hipparcos(path: &Path) -> io::Result<Vec<Star>> {
    load_hipparcos_with_limits(path, CatalogLimits::default())
}
pub fn load_hipparcos_with_limits(path: &Path, limits: CatalogLimits) -> io::Result<Vec<Star>> {
    at_path(
        path,
        read_bounded(path, limits.max_file_bytes).and_then(|bytes| parse_hipparcos(&bytes, limits)),
    )
}
pub fn parse_hipparcos(bytes: &[u8], limits: CatalogLimits) -> io::Result<Vec<Star>> {
    let mut r = Reader::new(bytes);
    let (records, tail) = bytes.as_chunks::<HIP_RECORD_BYTES>();
    let count = records.len();
    if bytes.len() as u64 > limits.max_file_bytes || count > limits.max_records {
        return Err(r.invalid("HIP catalog exceeds configured limits"));
    }
    if !tail.is_empty() {
        return Err(r.invalid("HIP catalog length must be a multiple of 45"));
    }
    let mut stars = Vec::with_capacity(count);
    for (index, raw) in records.iter().enumerate() {
        stars.push(Star {
            source_row: index + 1,
            raw: *raw,
            hip: r.i32()?,
            ra_deg: r.f32()?,
            dec_deg: r.f32()?,
            parallax_mas: r.f32()?,
            parallax_error_mas: r.f32()?,
            proper_motion_ra_mas_per_year: r.f32()?,
            proper_motion_dec_mas_per_year: r.f32()?,
            magnitude: r.f32()?,
            bv: r.f32()?,
            spectral_type: r.u8()? as i8,
            spectral_subtype: r.u8()? as i8,
            is_double: match r.u8()? {
                0 => false,
                1 => true,
                _ => return Err(r.invalid("double-star flag must be 0 or 1")),
            },
            flamsteed_number: r.i16()?,
            bayer_id: r.u8()? as i8,
            bayer_subscript: r.u8()? as i8,
            constellation_id: r.u8()? as i8,
            ads_component: r.u8()? as i8,
        });
    }
    Ok(stars)
}

pub fn load_constellations(path: &Path) -> io::Result<Vec<Constellation>> {
    load_constellations_with_limits(path, CatalogLimits::default())
}
pub fn load_constellations_with_limits(
    path: &Path,
    limits: CatalogLimits,
) -> io::Result<Vec<Constellation>> {
    at_path(
        path,
        read_bounded(path, limits.max_file_bytes)
            .and_then(|bytes| parse_constellations(&bytes, limits)),
    )
}
pub fn parse_constellations(bytes: &[u8], limits: CatalogLimits) -> io::Result<Vec<Constellation>> {
    let mut r = Reader::new(bytes);
    if bytes.len() as u64 > limits.max_file_bytes {
        return Err(r.invalid("constellation catalog exceeds byte limit"));
    }
    let mut records = Vec::new();
    while r.remaining() > 0 {
        if records.len() == limits.max_records {
            return Err(r.invalid("constellation catalog exceeds record limit"));
        }
        let start = r.offset;
        let abbreviation = r.name()?;
        let boundary_abbreviation = r.name()?;
        let english_name = r.name()?;
        let english_genitive = r.name()?;
        let japanese_hiragana_bytes = r.name()?;
        let japanese_kanji_bytes = r.name()?;
        let ra_deg = r.f32()?;
        let dec_deg = r.f32()?;
        let hip_lines = (0..r.u8()?)
            .map(|_| {
                let len = usize::from(r.u8()?);
                if len > r.remaining() / 4 {
                    return Err(r.invalid("truncated constellation line"));
                }
                (0..len).map(|_| r.i32()).collect()
            })
            .collect::<io::Result<Vec<_>>>()?;
        let len = usize::try_from(r.i16()?).map_err(|_| r.invalid("negative boundary count"))?;
        if len > r.remaining() / 8 {
            return Err(r.invalid("truncated constellation boundary"));
        }
        let boundary = (0..len)
            .map(|_| Ok([r.f32()?, r.f32()?]))
            .collect::<io::Result<Vec<_>>>()?;
        records.push(Constellation {
            source_row: records.len() + 1,
            raw: bytes[start..r.offset].to_vec(),
            abbreviation,
            boundary_abbreviation,
            english_name,
            english_genitive,
            japanese_hiragana_bytes,
            japanese_kanji_bytes,
            ra_deg,
            dec_deg,
            hip_lines,
            boundary,
        });
    }
    Ok(records)
}
