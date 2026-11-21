//! Plus の CAB 線画。Mitaka_plus f9fe624 の constellation_art.cpp と同じ極分岐と固定座標変換。
//! 原本および数値の由来はルートの licenses/Mitaka.txt を参照。
use crate::{
    astronomy::equatorial_coordinates,
    binary::{Reader, at_path, read_bounded},
    catalog::CatalogLimits,
};
use std::{io, path::Path};

#[derive(Debug)]
pub struct Art {
    pub coordinates: Vec<[f64; 2]>,
    /// 負の index はペンを離す指示。
    pub indices: Vec<i32>,
}

pub fn parse(bytes: &[u8], limits: CatalogLimits) -> io::Result<Art> {
    let mut r = Reader::new(bytes);
    if bytes.len() as u64 > limits.max_file_bytes {
        return Err(r.invalid("CAB byte limit"));
    }
    let header = [r.f32()?, r.f32()?, r.f32()?, r.f32()?, r.f32()?].map(f64::from);
    let [hx, hy, hz, scale, theta] = header;
    let norm = hx.hypot(hy).hypot(hz);
    if norm == 0. {
        return Err(r.invalid("zero CAB direction"));
    }
    let [hx, hy, hz] = [hx, hy, hz].map(|x| x / norm);
    let (sin, cos) = theta.to_radians().sin_cos();
    let (sf, cf) = (23.43870583_f64 - f64::from(23.4_f32))
        .to_radians()
        .sin_cos();
    let count = |r: &mut Reader, width| -> io::Result<usize> {
        let n = u32::from_le_bytes(r.array()?) as usize;
        if n > limits.max_records || n > r.remaining() / width {
            return Err(r.invalid("invalid CAB count"));
        }
        Ok(n)
    };
    let n = count(&mut r, 8)?;
    let mut coordinates = Vec::with_capacity(n);
    for _ in 0..n {
        let (u, v) = (f64::from(r.f32()?), f64::from(r.f32()?));
        let (x, y) = (-u * cos - v * sin, -u * sin + v * cos);
        let threshold = f64::from(0.99999_f32);
        let rotated = if hz > threshold {
            [x, y, 0.]
        } else if hz < -threshold {
            [x, -y, 0.]
        } else {
            let s = ((1. - hz) / 2.).sqrt() / hx.hypot(hy);
            let (a, b, w) = (-hy * s, hx * s, ((1. + hz) / 2.).sqrt());
            [
                (1. - 2. * b * b) * x + 2. * a * b * y,
                2. * a * b * x + (1. - 2. * a * a) * y,
                -2. * w * b * x + 2. * w * a * y,
            ]
        };
        let [x, y, z] = [
            hx + 2. * scale * rotated[0],
            hy + 2. * scale * rotated[1],
            hz + 2. * scale * rotated[2],
        ];
        coordinates.push(equatorial_coordinates([
            x,
            cf * y - sf * z,
            sf * y + cf * z,
        ])?);
    }
    let n = count(&mut r, 4)?;
    let mut indices = Vec::with_capacity(n);
    for _ in 0..n {
        let idx = r.i32()?;
        if idx >= 0 && idx as usize >= coordinates.len() {
            return Err(r.invalid("CAB point out of range"));
        }
        indices.push(idx);
    }
    if r.remaining() != 0 {
        return Err(r.invalid("trailing CAB bytes"));
    }
    Ok(Art {
        coordinates,
        indices,
    })
}
pub fn load(path: &Path, limits: CatalogLimits) -> io::Result<Art> {
    at_path(
        path,
        read_bounded(path, limits.max_file_bytes).and_then(|b| parse(&b, limits)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cab_center_break_and_invalid_index() {
        let mut bytes = vec![];
        for n in [1_f32, 0., 0., 1., 0.] {
            bytes.extend(n.to_le_bytes());
        }
        bytes.extend(1u32.to_le_bytes());
        bytes.extend([0; 8]);
        bytes.extend(2u32.to_le_bytes());
        bytes.extend(0_i32.to_le_bytes());
        bytes.extend((-1_i32).to_le_bytes());
        let art = parse(&bytes, CatalogLimits::default()).unwrap();
        assert_eq!(art.coordinates, [[0., 0.]]);
        assert_eq!(art.indices, [0, -1]);
        let end = bytes.len();
        bytes[end - 4..].copy_from_slice(&1_i32.to_le_bytes());
        assert!(parse(&bytes, CatalogLimits::default()).is_err());
    }
}
