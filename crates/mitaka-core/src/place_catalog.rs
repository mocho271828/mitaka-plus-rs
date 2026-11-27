//! Plus の Places 宣言をデータとして解析する。Lua は実行しない。
//! Mitaka_plus f9fe624 の place_catalog.cpp と observer_places.cpp を参照。
use crate::{
    binary::{at_path, read_bounded},
    catalog::CatalogLimits,
};
use std::{collections::HashMap, io, path::Path};

#[derive(Debug, Clone)]
pub struct Place {
    pub id: Vec<u8>,
    pub name: Vec<u8>,
    pub longitude: f64,
    pub latitude: f64,
    pub label_group: Vec<u8>,
    pub landscape: Vec<u8>,
}
impl Place {
    fn canonical(&self) -> bool {
        let mut parts = self.id.split(|b| *b == b'.');
        let group = parts.next();
        let object = parts.next();
        let place = parts.next();
        let supported = match (group, object) {
            (Some(b"Planet"), Some(n)) => matches!(
                n,
                b"Mercury"
                    | b"Venus"
                    | b"Earth"
                    | b"Mars"
                    | b"Jupiter"
                    | b"Saturn"
                    | b"Uranus"
                    | b"Neptune"
                    | b"Pluto"
            ),
            (Some(b"Satellite"), Some(b"Moon")) => true,
            _ => false,
        };
        supported && place.is_some_and(|p| !p.is_empty()) && parts.next().is_none()
    }
    pub fn earth(&self) -> bool {
        self.id
            .strip_prefix(b"Planet.Earth.")
            .is_some_and(|tail| !tail.is_empty() && !tail.contains(&b'.'))
    }
    pub fn longitude_signed(&self) -> f64 {
        (self.longitude + 180.).rem_euclid(360.) - 180.
    }
}
#[derive(Debug, Default)]
pub struct PlaceCatalog {
    pub places: Vec<Place>,
    pub landscapes: Vec<Landscape>,
    pub groups: Vec<LabelGroup>,
}
#[derive(Debug)]
pub struct Landscape {
    pub body: Vec<u8>,
    pub name: Vec<u8>,
    pub path: Vec<u8>,
    pub scale: f64,
    pub offsets: [f64; 2],
}
#[derive(Debug)]
pub struct LabelGroup {
    pub name: Vec<u8>,
    pub distance: f64,
    pub size: f64,
    pub rgba: [f64; 4],
}
enum Entry {
    Place(Place),
    Landscape(Landscape),
    Group(LabelGroup),
}
struct Line<'a>(&'a [u8]);
type Parsed<T> = Result<T, &'static str>;
impl<'a> Line<'a> {
    fn trim(&mut self) {
        while self.0.first().is_some_and(u8::is_ascii_whitespace) {
            self.0 = &self.0[1..];
        }
    }
    fn consume(&mut self, token: &[u8]) -> bool {
        self.trim();
        if let Some(rest) = self.0.strip_prefix(token) {
            self.0 = rest;
            true
        } else {
            false
        }
    }
    fn expect(&mut self, token: &[u8]) -> Parsed<()> {
        self.consume(token)
            .then_some(())
            .ok_or("unexpected Places token")
    }
    fn end(&mut self) -> Parsed<bool> {
        self.trim();
        if self.0.starts_with(b"--[") {
            return Err("long comments are unsupported");
        }
        Ok(self.0.is_empty() || self.0.starts_with(b"--"))
    }
    fn string(&mut self) -> Parsed<Vec<u8>> {
        self.trim();
        let (&quote, mut rest) = self.0.split_first().ok_or("missing string")?;
        if !matches!(quote, b'\'' | b'"') {
            return Err("expected quoted string");
        }
        let mut out = vec![];
        while let Some((&b, tail)) = rest.split_first() {
            rest = tail;
            if b == quote {
                self.0 = rest;
                return Ok(out);
            }
            if matches!(b,0x81..=0x9f|0xe0..=0xfc)
                && rest
                    .first()
                    .is_some_and(|c| matches!(c,0x40..=0x7e|0x80..=0xfc))
            {
                out.extend([b, rest[0]]);
                rest = &rest[1..];
                continue;
            }
            if b == b'\\' {
                let (&next, tail) = rest.split_first().ok_or("incomplete escape")?;
                if next != quote && next != b'\\' {
                    return Err("unsupported escape");
                }
                out.push(next);
                rest = tail;
            } else {
                out.push(b);
            }
        }
        Err("unterminated string")
    }
    fn number(&mut self) -> Parsed<f64> {
        self.trim();
        let n = self
            .0
            .iter()
            .take_while(|&&b| matches!(b, b'0'..=b'9' | b'+' | b'-' | b'.' | b'e' | b'E'))
            .count();
        let (raw, rest) = self.0.split_at(n);
        self.0 = rest;
        std::str::from_utf8(raw)
            .ok()
            .and_then(|t| t.parse::<f64>().ok())
            .filter(|n| n.is_finite())
            .ok_or("invalid number")
    }
    fn next_num(&mut self) -> Parsed<f64> {
        self.expect(b",")?;
        self.number()
    }
    fn next_str(&mut self) -> Parsed<Vec<u8>> {
        self.expect(b",")?;
        self.string()
    }
    fn nonempty(v: Vec<u8>) -> Parsed<Vec<u8>> {
        if v.is_empty() {
            Err("missing required name")
        } else {
            Ok(v)
        }
    }
    fn entry(&mut self) -> Parsed<Entry> {
        self.expect(b"Places")?;
        self.expect(b":")?;
        let place = if self.consume(b"addPlace") {
            self.expect(b"(")?;
            let id = Self::nonempty(self.string()?)?;
            let name = Self::nonempty(self.next_str()?)?;
            let longitude = self.next_num()?;
            let latitude = self.next_num()?;
            if !(-90. ..=90.).contains(&latitude) {
                return Err("invalid latitude");
            }
            let label_group = self.next_str()?;
            let landscape = self.next_str()?;
            Entry::Place(Place {
                id,
                name,
                longitude,
                latitude,
                label_group,
                landscape,
            })
        } else if self.consume(b"addLabelGroup") {
            self.expect(b"(")?;
            let name = Self::nonempty(self.string()?)?;
            let distance = self.next_num()?;
            let size = self.next_num()?;
            let mut rgba = [0.; 4];
            if distance < 0. || size < 0. {
                return Err("invalid label scale");
            }
            for channel in &mut rgba {
                *channel = self.next_num()?;
                if !(0. ..=1.).contains(channel) {
                    return Err("invalid RGBA");
                }
            }
            Entry::Group(LabelGroup {
                name,
                distance,
                size,
                rgba,
            })
        } else if self.consume(b"addLandscape") {
            self.expect(b"(")?;
            let body = Self::nonempty(self.string()?)?;
            let name = Self::nonempty(self.next_str()?)?;
            let path = Self::nonempty(self.next_str()?)?;
            let scale = self.next_num()?;
            if scale <= 0. {
                return Err("invalid landscape scale");
            }
            let offsets = [self.next_num()?, self.next_num()?];
            Entry::Landscape(Landscape {
                body,
                name,
                path,
                scale,
                offsets,
            })
        } else {
            return Err("unsupported Places method");
        };
        self.expect(b")")?;
        self.consume(b";");
        if !self.end()? {
            return Err("unexpected text after Places call");
        }
        Ok(place)
    }
}

pub fn parse(bytes: &[u8], limits: CatalogLimits) -> io::Result<PlaceCatalog> {
    let mut result = PlaceCatalog::default();
    let mut by_id = HashMap::new();
    let mut entries = 0;
    if bytes.len() as u64 > limits.max_file_bytes {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Places byte limit",
        ));
    }
    for (index, raw) in bytes.split(|&c| c == b'\n').enumerate() {
        let err = |msg| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("line {}: {msg}", index + 1),
            )
        };
        if raw.len() > 4096 || raw.contains(&0) {
            return Err(err("NUL or oversized Places line"));
        }
        let mut line = Line(raw);
        if line.end().map_err(err)? {
            continue;
        }
        entries += 1;
        if entries > limits.max_records {
            return Err(err("Places record limit"));
        }
        match line.entry().map_err(err)? {
            Entry::Landscape(l) => result.landscapes.push(l),
            Entry::Group(g) => result.groups.push(g),
            Entry::Place(place) => {
                if place.canonical() {
                    if let Some(&i) = by_id.get(&place.id) {
                        result.places[i] = place;
                        continue;
                    }
                    by_id.insert(place.id.clone(), result.places.len());
                }
                result.places.push(place);
            }
        }
    }
    Ok(result)
}
pub fn load_observer_places(root: &Path) -> io::Result<PlaceCatalog> {
    let limits = CatalogLimits::default();
    for name in [
        "places_jp.dat",
        "places.dat",
        "defaultplaces_jp.dat",
        "defaultplaces.dat",
    ] {
        let path = root.join(name);
        match read_bounded(&path, limits.max_file_bytes) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
            r => return at_path(&path, r.and_then(|b| parse(&b, limits))),
        }
    }
    Ok(Default::default())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn replaces_duplicate_earth_ids_and_rejects_nonliteral_input() {
        let data = br#"Places:addPlace("Planet.Earth.A","old",0,0,"","");
Places:addPlace("Planet.Earth.A","new",220,42,"",""); -- duplicate"#;
        let rows = parse(data, CatalogLimits::default()).unwrap().places;
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, b"new");
        assert_eq!(rows[0].longitude_signed(), -140.);
        assert!(
            parse(
                b"Places:addPlace('a','b',1+2,3,'','');",
                CatalogLimits::default()
            )
            .is_err()
        );
    }
}
