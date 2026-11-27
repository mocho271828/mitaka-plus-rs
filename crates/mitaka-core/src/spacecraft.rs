//! Spacecraft definitions, registration lists and trajectory snapshots, following
//! Mitaka_plus f9fe624 spacecraft_trajectory{,_parser}.cpp and spacecraft_position.cpp.
mod trajectory;
pub use trajectory::{Ppi, Pth, Sample, SampledPosition, Sampling, Segment};

use crate::{binary::read_bounded, catalog::CatalogLimits};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    fs, io,
    ops::Range,
    path::{Path, PathBuf},
};
fn invalid(reason: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, reason)
}

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub catalog: CatalogLimits,
    pub max_total_bytes: u64,
    pub max_sources: usize,
    pub max_fields: usize,
    pub max_line_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            catalog: CatalogLimits::default(),
            max_total_bytes: 512 << 20,
            max_sources: 10000,
            max_fields: 1000000,
            max_line_bytes: 65536,
        }
    }
}
#[derive(Debug)]
pub struct Field {
    pub key: Vec<u8>,
    pub value: Vec<u8>,
    pub section: Vec<u8>,
    pub line: usize,
    pub byte_range: Range<usize>,
}
#[derive(Debug)]
pub struct Definition {
    pub fields: Vec<Field>,
    pub trajectory: Option<usize>,
    pub valid: bool,
}
impl Definition {
    pub fn field(&self, key: &[u8], section: &[u8]) -> Option<&[u8]> {
        let mut fields = self
            .fields
            .iter()
            .filter(|f| f.key == key && f.section == section);
        let field = fields.next()?;
        fields.next().is_none().then_some(&field.value)
    }
}
#[derive(Debug)]
pub struct Registration {
    pub reference: Vec<u8>,
    pub line: usize,
    pub byte_range: Range<usize>,
    pub definition: Option<usize>,
}
#[derive(Debug)]
pub enum SourceData {
    Definition(Definition),
    Registration(Vec<Registration>),
    Pth(Pth),
    Ppi(Ppi),
}
#[derive(Debug)]
pub struct Source {
    pub key: String,
    pub raw: Vec<u8>,
    pub sha256: Option<[u8; 32]>,
    pub data: Option<SourceData>,
    pub error: Option<io::Error>,
    pub diagnostics: Vec<String>,
}
impl Source {
    pub fn record_bytes(&self, ordinal: usize) -> Option<&[u8]> {
        let range = match self.data.as_ref()? {
            SourceData::Definition(_) if ordinal == 0 => 0..self.raw.len(),
            SourceData::Registration(rows) => rows.get(ordinal)?.byte_range.clone(),
            SourceData::Pth(p) if ordinal < p.samples().len() => ordinal * 32..(ordinal + 1) * 32,
            SourceData::Ppi(p) if ordinal < p.segments().len() => {
                ordinal * 128..(ordinal + 1) * 128
            }
            _ => return None,
        };
        self.raw.get(range)
    }
}
#[derive(Debug, Default)]
pub struct Dataset {
    pub sources: Vec<Source>,
    pub directory_errors: Vec<(String, io::Error)>,
}
#[derive(Clone, Copy)]
enum Kind {
    Definition,
    Registration,
    Pth,
    Ppi,
}

fn discover(
    dir: &Path,
    prefix: &str,
    remaining: usize,
) -> io::Result<Vec<(String, PathBuf, Kind)>> {
    if !fs::symlink_metadata(dir)?.file_type().is_dir() {
        return Err(invalid("expected direct directory"));
    }
    let mut keys = Vec::new();
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        let Some(ext) = path.extension().and_then(|x| x.to_str()) else {
            continue;
        };
        let kind = match ext.to_ascii_lowercase().as_str() {
            "ppi" => Kind::Ppi,
            "pth" if prefix == "spacecraft" => Kind::Pth,
            "dat" if prefix == "spacecraft" => {
                if path
                    .file_name()
                    .and_then(|x| x.to_str())
                    .is_some_and(|s| s.eq_ignore_ascii_case("spacecraft.dat"))
                {
                    Kind::Registration
                } else {
                    Kind::Definition
                }
            }
            _ => continue,
        };
        if keys.len() == remaining {
            return Err(invalid("spacecraft source count limit exceeded"));
        }
        let name = path
            .file_name()
            .and_then(|x| x.to_str())
            .ok_or_else(|| invalid("non-UTF8 source key"))?;
        keys.push((format!("{prefix}/{name}"), path, kind));
    }
    Ok(keys)
}
struct Budget {
    bytes: u64,
    records: usize,
    fields: usize,
}
impl Dataset {
    pub fn load(data_dir: &Path, limits: Limits) -> Self {
        let mut result = Self::default();
        let spacecraft = data_dir
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("spacecraft");
        let mut keys = Vec::new();
        for (prefix, dir) in [("data", data_dir), ("spacecraft", &spacecraft)] {
            match discover(dir, prefix, limits.max_sources - keys.len()) {
                Ok(mut items) => keys.append(&mut items),
                Err(e) => result.directory_errors.push((prefix.to_owned(), e)),
            }
        }
        keys.sort_by(|a, b| a.0.cmp(&b.0));
        let mut budget = Budget {
            bytes: limits.max_total_bytes,
            records: limits.catalog.max_records,
            fields: limits.max_fields,
        };
        for (key, path, kind) in keys {
            let mut s = Source {
                key,
                raw: Vec::new(),
                sha256: None,
                data: None,
                error: None,
                diagnostics: Vec::new(),
            };
            s.error = s.load(&path, kind, limits, &mut budget).err();
            result.sources.push(s);
        }
        result.resolve();
        result
    }
    fn resolve(&mut self) {
        let mut trajectories = Keys::default();
        let mut definitions = Keys::default();
        for (ordinal, src) in self.sources.iter().enumerate() {
            match &src.data {
                Some(SourceData::Pth(_) | SourceData::Ppi(_)) => {
                    trajectories.add(&src.key, ordinal)
                }
                Some(SourceData::Definition(_)) => definitions.add(&src.key, ordinal),
                _ => {}
            }
        }
        for src in &mut self.sources {
            match &mut src.data {
                Some(SourceData::Definition(d)) => {
                    let mut refs = d.fields.iter().filter(|f| {
                        f.section.is_empty() && matches!(f.key.as_slice(), b"Orbit" | b"TRAJECTORY")
                    });
                    if let Some(f) = refs.next()
                        && refs.next().is_none()
                    {
                        d.trajectory =
                            trajectories.resolve(&src.key, &f.value, &mut src.diagnostics);
                    }
                    if d.trajectory.is_none() {
                        src.diagnostics.push("unresolved local trajectory".into());
                    }
                }
                Some(SourceData::Registration(entries)) => {
                    for e in entries {
                        e.definition =
                            definitions.resolve(&src.key, &e.reference, &mut src.diagnostics);
                        if e.definition.is_none() {
                            src.diagnostics
                                .push(format!("line {}: unresolved local definition", e.line));
                        }
                    }
                }
                _ => {}
            }
        }
    }
}
impl Source {
    fn load(
        &mut self,
        path: &Path,
        kind: Kind,
        limits: Limits,
        budget: &mut Budget,
    ) -> io::Result<()> {
        if !fs::symlink_metadata(path)?.file_type().is_file() {
            return Err(invalid("expected direct regular source file"));
        }
        self.raw = read_bounded(path, limits.catalog.max_file_bytes.min(budget.bytes))?;
        budget.bytes -= self.raw.len() as u64;
        self.sha256 = Some(Sha256::digest(&self.raw).into());
        let (data, count) = match kind {
            Kind::Pth => {
                let p = Pth::parse(&self.raw, budget.records)?;
                let n = p.samples().len();
                (SourceData::Pth(p), n)
            }
            Kind::Ppi => {
                let p = Ppi::parse(&self.raw, budget.records)?;
                let n = p.segments().len();
                (SourceData::Ppi(p), n)
            }
            Kind::Registration => {
                let mut entries = Vec::new();
                text_lines(
                    &self.raw,
                    limits.max_line_bytes,
                    |line, byte_range, text| {
                        if entries.len() >= budget.records {
                            return Err(invalid("aggregate spacecraft record limit exceeded"));
                        }
                        entries.push(Registration {
                            reference: text.to_vec(),
                            line,
                            byte_range,
                            definition: None,
                        });
                        Ok(())
                    },
                )?;
                let n = entries.len();
                (SourceData::Registration(entries), n)
            }
            Kind::Definition => {
                if budget.records == 0 {
                    return Err(invalid("aggregate spacecraft record limit exceeded"));
                }
                let d = parse_definition(
                    &self.raw,
                    limits.max_line_bytes,
                    budget.fields,
                    &mut self.diagnostics,
                )?;
                budget.fields -= d.fields.len();
                (SourceData::Definition(d), 1)
            }
        };
        budget.records -= count;
        self.data = Some(data);
        Ok(())
    }
}
fn text_lines<'a>(
    raw: &'a [u8],
    limit: usize,
    mut parse: impl FnMut(usize, Range<usize>, &'a [u8]) -> io::Result<()>,
) -> io::Result<()> {
    if raw.contains(&0) {
        return Err(invalid("NUL in spacecraft source"));
    }
    let mut offset = 0;
    for (i, line) in raw.split_inclusive(|v| *v == b'\n').enumerate() {
        let start = offset;
        offset += line.len();
        let content = line.strip_suffix(b"\n").unwrap_or(line);
        if content.len() > limit {
            return Err(invalid("spacecraft source line too long"));
        }
        let text = content.trim_ascii();
        let text = if i == 0 {
            text.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(text)
        } else {
            text
        };
        if text.is_empty() || text.starts_with(b"#") || text.starts_with(b"//") {
            continue;
        }
        parse(i + 1, start..offset, text)?;
    }
    Ok(())
}
fn parse_definition(
    raw: &[u8],
    line_limit: usize,
    fields_limit: usize,
    diag: &mut Vec<String>,
) -> io::Result<Definition> {
    let mut section = Vec::new();
    let mut keys = HashSet::new();
    let mut fields = Vec::new();
    let mut valid = true;
    text_lines(raw, line_limit, |line, byte_range, text| {
        if text.starts_with(b"[") {
            if text.len() < 3 || !text.ends_with(b"]") {
                return Err(invalid("malformed spacecraft section"));
            }
            section = text[1..text.len() - 1].to_vec();
            return Ok(());
        }
        let colon = text
            .iter()
            .position(|x| *x == b':')
            .ok_or_else(|| invalid("expected key: value"))?;
        let key = text[..colon].trim_ascii();
        let value = text[colon + 1..].trim_ascii();
        if key.is_empty() || fields.len() >= fields_limit {
            return Err(invalid("empty key or spacecraft field limit exceeded"));
        }
        if !keys.insert((section.clone(), key)) {
            valid = false;
            diag.push(format!("line {line}: duplicate field"));
        }
        match key {
            b"Size" | b"SCALE" => {
                if !std::str::from_utf8(value)
                    .ok()
                    .and_then(|s| s.parse::<f64>().ok())
                    .is_some_and(|n| n.is_finite())
                {
                    valid = false;
                    diag.push(format!("line {line}: invalid number"));
                }
            }
            b"Name" | b"DisplayName" | b"Model" | b"Orbit" | b"KEY" | b"MODEL" | b"OFFSET"
            | b"TRAJECTORY" | b"ORIGIN" | b"NAME" => {}
            _ => diag.push(format!("line {line}: uninterpreted field")),
        }
        fields.push(Field {
            key: key.to_vec(),
            value: value.to_vec(),
            section: section.clone(),
            line,
            byte_range,
        });
        Ok(())
    })?;
    Ok(Definition {
        fields,
        trajectory: None,
        valid,
    })
}
#[derive(Default)]
struct Keys {
    exact: HashMap<String, usize>,
    folded: HashMap<String, Option<usize>>,
}
impl Keys {
    fn add(&mut self, key: &str, ordinal: usize) {
        self.exact.insert(key.to_owned(), ordinal);
        self.folded
            .entry(key.to_ascii_lowercase())
            .and_modify(|v| *v = None)
            .or_insert(Some(ordinal));
    }
    fn resolve(&self, source: &str, reference: &[u8], diag: &mut Vec<String>) -> Option<usize> {
        let path = std::str::from_utf8(reference).ok()?;
        if path.is_empty() || path.starts_with('/') || path.contains(['\\', ':']) {
            return None;
        }
        let mut parts = source.rsplit_once('/')?.0.split('/').collect::<Vec<_>>();
        for c in path.split('/') {
            match c {
                "" | "." => {}
                ".." => {
                    parts.pop()?;
                }
                c => parts.push(c),
            }
        }
        let key = parts.join("/");
        if let Some(&i) = self.exact.get(&key) {
            return Some(i);
        }
        if let Some(&Some(i)) = self.folded.get(&key.to_ascii_lowercase()) {
            diag.push("unique case-insensitive reference".into());
            return Some(i);
        }
        None
    }
}
