use super::{Encoding, Kind, Orbit, Perihelion, Record, invalid};
use crate::{
    astronomy::{GregorianDateTime, julian_date},
    binary::Reader,
    orbit::OrbitalElements,
};
use std::{collections::BTreeMap, io, ops::Range};

const MEAN_MOTION_AT_ONE_AU: f64 = 0.985607669;
fn axis_from_motion(n: f64) -> f64 {
    let ratio = MEAN_MOTION_AT_ONE_AU / n;
    if ratio.is_finite() {
        ratio.powf(2. / 3.)
    } else {
        MEAN_MOTION_AT_ONE_AU.powf(2. / 3.) / n.powf(2. / 3.)
    }
}
fn number(input: &[u8]) -> io::Result<f64> {
    std::str::from_utf8(input.trim_ascii())
        .ok()
        .and_then(|v| v.parse::<f64>().ok())
        .filter(|v| v.is_finite())
        .ok_or_else(|| invalid("invalid finite number"))
}
fn integer(input: &[u8]) -> io::Result<i32> {
    let bytes = input.trim_ascii();
    if bytes.starts_with(b"+") {
        return Err(invalid("invalid int32"));
    }
    std::str::from_utf8(bytes)
        .ok()
        .and_then(|v| v.parse().ok())
        .ok_or_else(|| invalid("invalid int32"))
}
fn lines(raw: &[u8]) -> impl Iterator<Item = (Range<usize>, &[u8])> {
    raw.split_inclusive(|b| *b == b'\n')
        .scan(0, |offset, line| {
            let start = *offset;
            *offset += line.len();
            let content = line.strip_suffix(b"\n").unwrap_or(line);
            Some((
                start..*offset,
                content.strip_suffix(b"\r").unwrap_or(content),
            ))
        })
        .filter(|(_, line)| !matches!(line.trim_ascii().first(), None | Some(b'#' | b'%')))
}
pub(super) fn records(
    raw: &[u8],
    kind: Kind,
    encoding: Encoding,
    limit: usize,
) -> io::Result<Vec<Record>> {
    match encoding {
        Encoding::Packed36 | Encoding::Packed37 => packed(raw, kind, encoding, limit),
        Encoding::FixedComet => {
            let mut records = Vec::new();
            for (range, line) in lines(raw) {
                if records.len() == limit {
                    return Err(invalid("record limit exceeded"));
                }
                records.push(
                    fixed_comet(line, range.clone())
                        .map_err(|e| invalid(&format!("byte {}: {e}", range.start)))?,
                );
            }
            Ok(records)
        }
        Encoding::ElementText if limit > 0 => Ok(vec![elements(raw, kind)?]),
        _ => Err(invalid("unexpected encoding or record limit exceeded")),
    }
}
fn packed(raw: &[u8], kind: Kind, encoding: Encoding, limit: usize) -> io::Result<Vec<Record>> {
    let width = if encoding == Encoding::Packed36 {
        36
    } else {
        37
    };
    if !raw.len().is_multiple_of(width) || raw.len() / width > limit {
        return Err(invalid("incomplete packed input or record limit exceeded"));
    }
    let mut records = Vec::with_capacity(raw.len() / width);
    let mut r = Reader::new(raw);
    while r.remaining() > 0 {
        let start = r.offset;
        let mut item = Record::new(kind, encoding, start..start + width);
        item.number = Some(r.i32()?);
        if width == 37 {
            item.raw_flag = Some(r.u8()?);
        }
        let epoch = f64::from_le_bytes(r.array()?);
        let [m, w, node, i, e, n] =
            [r.f32()?, r.f32()?, r.f32()?, r.f32()?, r.f32()?, r.f32()?].map(f64::from);
        if !epoch.is_finite() || n <= 0. || e < 0. {
            return Err(invalid(&format!(
                "byte {start}: invalid epoch, eccentricity or mean motion"
            )));
        }
        item.epoch_value = Some(epoch);
        item.orbit = Orbit::Mean(OrbitalElements {
            epoch_jd: epoch,
            semi_major_axis_au: axis_from_motion(n),
            eccentricity: e,
            mean_anomaly_deg: m,
            mean_motion_deg_per_day: n,
            ascending_node_deg: node,
            inclination_deg: i,
            argument_of_periapsis_deg: w,
        });
        records.push(item);
    }
    Ok(records)
}

fn elements(raw: &[u8], kind: Kind) -> io::Result<Record> {
    let mut row = Record::new(kind, Encoding::ElementText, 0..raw.len());
    let mut fields = BTreeMap::new();
    for (range, line) in lines(raw) {
        let (key, value) = line
            .iter()
            .position(|v| *v == b':')
            .map(|i| (line[..i].trim_ascii(), line[i + 1..].trim_ascii()))
            .ok_or_else(|| invalid(&format!("byte {}: missing separator", range.start)))?;
        if key.is_empty() || fields.insert(key, value).is_some() {
            return Err(invalid(&format!(
                "byte {}: empty or duplicate element key",
                range.start
            )));
        }
        row.fields.push((key.to_vec(), value.to_vec()));
    }
    let field = |key: &[u8]| {
        fields
            .get(key)
            .copied()
            .ok_or_else(|| invalid(&format!("missing field {}", String::from_utf8_lossy(key))))
    };
    let num = |key: &[u8]| number(field(key)?);
    row.name_bytes = field(b"Name")?.to_vec();
    row.display_name_bytes = fields
        .get(b"DisplayName".as_slice())
        .copied()
        .unwrap_or_default()
        .to_vec();
    let e = num(b"e")?;
    let w = num(b"w")?;
    let node = num(b"node")?;
    let i = num(b"i")?;
    if e < 0. {
        return Err(invalid("negative eccentricity"));
    }
    if kind == Kind::Comet {
        if num(b"q")? <= 0. {
            return Err(invalid("invalid perihelion distance"));
        }
        row.perihelion_time_value = Some(num(b"Tp")?);
        return Ok(row);
    }
    row.kind = match field(b"Type")? {
        b"Asteroid" => Kind::Asteroid,
        b"TNObject" => Kind::TransNeptunian,
        _ => return Err(invalid("unknown small-body Type")),
    };
    row.number = Some(integer(field(b"No")?)?);
    let epoch = num(b"Epoch")?;
    let n = fields.get(b"n".as_slice()).map(|v| number(v)).transpose()?;
    if n.is_some_and(|v| v <= 0.) {
        return Err(invalid("invalid explicit mean motion"));
    }
    let a = match fields.get(b"a".as_slice()) {
        Some(v) => number(v)?,
        None => axis_from_motion(n.ok_or_else(|| invalid("missing a or n"))?),
    };
    let motion = n.unwrap_or_else(|| MEAN_MOTION_AT_ONE_AU / a / a.sqrt());
    if !a.is_finite() || a <= 0. || !motion.is_finite() || motion <= 0. {
        return Err(invalid("invalid axis or mean motion"));
    }
    row.epoch_value = Some(epoch);
    row.orbit = Orbit::Mean(OrbitalElements {
        epoch_jd: epoch,
        semi_major_axis_au: a,
        eccentricity: e,
        mean_anomaly_deg: num(b"M0")?,
        mean_motion_deg_per_day: motion,
        ascending_node_deg: node,
        inclination_deg: i,
        argument_of_periapsis_deg: w,
    });
    Ok(row)
}
fn calendar(text: &[u8]) -> io::Result<Option<f64>> {
    if number(text)? < 15821015. {
        return Ok(None);
    }
    let split = text.iter().position(|b| *b == b'.').unwrap_or(text.len());
    if split != 8 || !text[..split].iter().all(u8::is_ascii_digit) {
        return Err(invalid("invalid calendar perihelion"));
    }
    let date = integer(&text[..split])?;
    let day = julian_date(GregorianDateTime {
        year: date / 10000,
        month: ((date / 100) % 100) as u8,
        day: (date % 100) as u8,
        hour: 0,
        minute: 0,
        second: 0.,
    })?;
    let fraction = if split < text.len() {
        number(&text[split..])?
    } else {
        0.
    };
    if !(0.0..1.).contains(&fraction) {
        return Err(invalid("invalid fractional perihelion date"));
    }
    Ok(Some(day + fraction))
}
fn fixed_comet(line: &[u8], range: Range<usize>) -> io::Result<Record> {
    if line.len() < 115 {
        return Err(invalid("short fixed-width comet row"));
    }
    let mut row = Record::new(Kind::Comet, Encoding::FixedComet, range);
    let fields: [(&[u8], Range<usize>); 9] = [
        (b"Name", 0..38),
        (b"Epoch", 39..46),
        (b"q", 47..58),
        (b"e", 59..69),
        (b"i", 70..79),
        (b"w", 80..89),
        (b"Node", 90..99),
        (b"Tp", 100..114),
        (b"Ref", 115..line.len()),
    ];
    row.fields = fields
        .iter()
        .map(|(key, range)| (key.to_vec(), line[range.clone()].trim_ascii().to_vec()))
        .collect();
    let field = |i: usize| row.fields[i].1.as_slice();
    if field(0).is_empty() {
        return Err(invalid("empty comet name"));
    }
    row.name_bytes = field(0).to_vec();
    row.epoch_value = Some(number(field(1))?);
    let q = number(field(2))?;
    let e = number(field(3))?;
    let i = number(field(4))?;
    let w = number(field(5))?;
    let node = number(field(6))?;
    if q <= 0. || e < 0. {
        return Err(invalid("invalid fixed-width comet q or e"));
    }
    row.perihelion_time_value = Some(number(field(7))?);
    if let Some(jd) = calendar(field(7))? {
        row.orbit = Orbit::Perihelion(Perihelion {
            distance_au: q,
            eccentricity: e,
            jd,
            ascending_node_deg: node,
            inclination_deg: i,
            argument_deg: w,
        });
    }
    Ok(row)
}
pub(super) fn aliases(raw: &[u8]) -> io::Result<BTreeMap<i32, Vec<Vec<u8>>>> {
    let mut result = BTreeMap::new();
    for (range, line) in lines(raw) {
        let text = line.trim_ascii();
        let i = text
            .iter()
            .position(|b| matches!(b, b' ' | b'\t'))
            .ok_or_else(|| invalid(&format!("byte {}: missing alias", range.start)))?;
        let name = text[i..].trim_ascii();
        if name.is_empty() {
            return Err(invalid("empty alias"));
        }
        result
            .entry(integer(&text[..i])?)
            .or_insert_with(Vec::new)
            .push(name.to_vec());
    }
    Ok(result)
}
