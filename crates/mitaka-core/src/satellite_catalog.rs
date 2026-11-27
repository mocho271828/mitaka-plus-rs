//! Byte-preserving satellite system blocks and orbital source files, following
//! Mitaka_plus f9fe624 satellite_catalog{,_parser}.cpp. Source ordinals are zero-based.
use crate::{binary::read_bounded, catalog::CatalogLimits, satellite::SatelliteElements};
use std::{
    collections::{BTreeMap, HashSet},
    fs, io,
    path::{Component, Path, PathBuf},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceKind {
    System,
    Orbit,
}
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub catalog: CatalogLimits,
    pub max_total_bytes: u64,
    pub max_sources: usize,
    pub max_fields: usize,
    pub max_line_bytes: usize,
    pub max_depth: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            catalog: CatalogLimits::default(),
            max_total_bytes: 512 * 1024 * 1024,
            max_sources: 10000,
            max_fields: 1_000_000,
            max_line_bytes: 65_536,
            max_depth: 64,
        }
    }
}
#[derive(Debug, Clone)]
pub struct Field {
    pub key: String,
    pub value_bytes: Vec<u8>,
    pub byte_offset: usize,
    pub byte_len: usize,
    pub line: usize,
    pub numbers: Option<Vec<f64>>,
}
#[derive(Debug)]
pub struct Record {
    pub block: String,
    pub ordinal: usize,
    pub parent: Option<usize>,
    pub byte_offset: usize,
    pub byte_len: usize,
    pub line: usize,
    pub fields: Vec<Field>,
    pub orbit_source: Option<usize>,
}
impl Record {
    pub fn field(&self, key: &str) -> Option<&Field> {
        let mut hits = self.fields.iter().filter(|v| v.key == key);
        let first = hits.next()?;
        hits.next().is_none().then_some(first)
    }
    pub fn scalar(&self, key: &str) -> Option<f64> {
        match self.field(key)?.numbers.as_deref()? {
            [v] => Some(*v),
            _ => None,
        }
    }
    pub fn name(&self) -> Option<&[u8]> {
        self.field("Name")?
            .value_bytes
            .split(u8::is_ascii_whitespace)
            .find(|v| !v.is_empty())
    }
    pub fn elements(&self) -> Option<SatelliteElements> {
        Some(SatelliteElements {
            epoch_jd: self.scalar("Epoch")?,
            semi_major_axis_km: self.scalar("a")?,
            eccentricity: self.scalar("e")?,
            mean_anomaly_deg: self.scalar("M0")?,
            mean_motion_deg_per_day: self.scalar("n")?,
            argument_of_periapsis_deg: self.scalar("w")?,
            inclination_deg: self.scalar("i")?,
            raw_node_deg: self.scalar("node")?,
            periapsis_period_years: self.scalar("Pw")?,
            node_period_years: self.scalar("Pnode")?,
            laplace_pole_deg: self
                .scalar("RA")
                .zip(self.scalar("Decl"))
                .map(|(r, d)| [r, d]),
        })
    }
}
#[derive(Debug)]
pub struct Diagnostic {
    pub source: String,
    pub line: usize,
    pub offset: usize,
    pub message: String,
}
#[derive(Debug)]
pub struct Source {
    pub key: String,
    pub kind: SourceKind,
    pub raw: Vec<u8>,
    pub read_complete: bool,
    pub parsed: bool,
    pub valid: bool,
    pub records: Vec<Record>,
}
#[derive(Debug, Default)]
pub struct Dataset {
    pub sources: Vec<Source>,
    pub diagnostics: Vec<Diagnostic>,
}
struct Parsed {
    records: Vec<Record>,
    diagnostics: Vec<Diagnostic>,
    fields: usize,
    valid: bool,
}
fn trim(bytes: &[u8]) -> &[u8] {
    let space = |v: &u8| matches!(v, b' ' | b'\t' | b'\r');
    let a = bytes.iter().position(|v| !space(v)).unwrap_or(bytes.len());
    let b = bytes.iter().rposition(|v| !space(v)).map_or(a, |x| x + 1);
    &bytes[a..b]
}
fn key(bytes: &[u8]) -> Option<&str> {
    if bytes.is_empty()
        || !bytes
            .iter()
            .all(|v| v.is_ascii_alphanumeric() || *v == b'_')
    {
        return None;
    }
    std::str::from_utf8(bytes).ok()
}
fn issue(source: &str, line: usize, offset: usize, message: impl Into<String>) -> Diagnostic {
    Diagnostic {
        source: source.to_owned(),
        line,
        offset,
        message: message.into(),
    }
}
fn record(name: &str, ordinal: usize, parent: Option<usize>, offset: usize, line: usize) -> Record {
    Record {
        block: name.into(),
        ordinal,
        parent,
        byte_offset: offset,
        line,
        byte_len: 0,
        fields: vec![],
        orbit_source: None,
    }
}
fn parse(
    key_name: &str,
    kind: SourceKind,
    bytes: &[u8],
    limits: Limits,
    max_records: usize,
    max_fields: usize,
) -> Result<Parsed, Diagnostic> {
    let mut result = Parsed {
        records: vec![],
        diagnostics: vec![],
        fields: 0,
        valid: true,
    };
    let mut stack: Vec<usize> = vec![];
    let mut pending: Option<(&str, usize, usize)> = None;
    let mut seen: Vec<HashSet<String>> = vec![];
    let mut next = 0;
    let mut line = 0;
    for raw in bytes.split_inclusive(|&c| c == b'\n') {
        line += 1;
        let offset = next;
        next += raw.len();
        let err = |why| issue(key_name, line, offset, why);
        if raw.len() > limits.max_line_bytes || raw.contains(&0) {
            return Err(err("invalid satellite text line"));
        }
        let text = trim(raw.strip_suffix(b"\n").unwrap_or(raw));
        if text.is_empty() || text[0] == b'#' {
            continue;
        }
        if kind == SourceKind::System {
            if text == b"{" {
                let (name, offset, start) = pending
                    .take()
                    .ok_or_else(|| err("opening brace without block name"))?;
                if stack.len() >= limits.max_depth || result.records.len() >= max_records {
                    return Err(err("block depth or count limit exceeded"));
                }
                if !matches!(name, "Ssystem" | "Sun" | "Planet" | "Satellite") {
                    result.diagnostics.push(issue(
                        key_name,
                        start,
                        offset,
                        "unknown block preserved",
                    ));
                }
                let i = result.records.len();
                result
                    .records
                    .push(record(name, i, stack.last().copied(), offset, start));
                stack.push(i);
                seen.push(HashSet::new());
                continue;
            }
            if text == b"}" {
                if pending.is_some() {
                    return Err(err("closing brace after pending block name"));
                }
                let i = stack.pop().ok_or_else(|| err("unmatched closing brace"))?;
                let r = &mut result.records[i];
                r.byte_len = next - r.byte_offset;
                continue;
            }
            if pending.is_some() {
                return Err(err("expected opening brace"));
            }
            if !text.contains(&b':') {
                pending = Some((
                    key(text).ok_or_else(|| err("invalid block name"))?,
                    offset,
                    line,
                ));
                continue;
            }
        } else if result.records.is_empty() {
            if max_records == 0 {
                return Err(err("orbit record limit exceeded"));
            }
            let mut orbit = record("Orbit", 0, None, 0, 1);
            orbit.byte_len = bytes.len();
            result.records.push(orbit);
            seen.push(HashSet::new());
        }
        let i = if kind == SourceKind::System {
            *stack.last().ok_or_else(|| err("field outside block"))?
        } else {
            0
        };
        let colon = text
            .iter()
            .position(|&c| c == b':')
            .ok_or_else(|| err("expected key: value"))?;
        let name = key(trim(&text[..colon])).ok_or_else(|| err("invalid field key"))?;
        let value = trim(&text[colon + 1..]);
        if result.fields == max_fields {
            return Err(err("field limit exceeded"));
        }
        result.fields += 1;
        if !seen[i].insert(name.into()) {
            result.valid = false;
            result
                .diagnostics
                .push(err("ambiguous duplicate field preserved"));
        }
        let numeric = match kind {
            SourceKind::Orbit => matches!(
                name,
                "Epoch"
                    | "a"
                    | "e"
                    | "w"
                    | "M0"
                    | "i"
                    | "node"
                    | "n"
                    | "P"
                    | "Pw"
                    | "Pnode"
                    | "RA"
                    | "Decl"
                    | "T"
                    | "L"
            ),
            SourceKind::System => {
                matches!(name, "Radius" | "Magnitude" | "Color" | "Visible" | "Ring")
            }
        };
        let known = numeric
            || match kind {
                SourceKind::Orbit => matches!(name, "Planet" | "RefPlane"),
                SourceKind::System => matches!(name, "Name" | "Orbit" | "Texture" | "RingTexture"),
            };
        if !known {
            result.diagnostics.push(err("unknown field preserved"));
        }
        let numbers = if numeric {
            let values = std::str::from_utf8(value).ok().and_then(|t| {
                t.split_ascii_whitespace()
                    .map(|v| v.parse::<f64>().ok().filter(|v| v.is_finite()))
                    .collect::<Option<Vec<_>>>()
            });
            let scalar = kind == SourceKind::Orbit || matches!(name, "Magnitude" | "Visible");
            let values = values.filter(|n| !n.is_empty() && (!scalar || n.len() == 1));
            if values.is_none() {
                result.valid = false;
                result
                    .diagnostics
                    .push(err("invalid numeric field preserved"));
            }
            values
        } else {
            None
        };
        if kind == SourceKind::Orbit
            && name == "RefPlane"
            && !matches!(value, b"L" | b"E" | b"Q" | b"C" | b"I")
        {
            result.diagnostics.push(err("unresolved reference plane"));
        }
        result.records[i].fields.push(Field {
            key: name.to_owned(),
            value_bytes: value.to_vec(),
            byte_offset: offset,
            byte_len: raw.len(),
            line,
            numbers,
        });
    }
    if pending.is_some() || !stack.is_empty() {
        return Err(issue(key_name, line + 1, bytes.len(), "unclosed block"));
    }
    Ok(result)
}
fn discover(dir: &Path, kind: SourceKind, limit: usize) -> io::Result<(Vec<PathBuf>, bool)> {
    if !fs::symlink_metadata(dir)?.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "satellite source must be a direct directory",
        ));
    }
    let mut paths = Vec::new();
    for entry in fs::read_dir(dir)? {
        let e = entry?;
        let p = e.path();
        let typ = e.file_type()?;
        if !typ.is_file() && !typ.is_symlink() {
            continue;
        }
        if kind == SourceKind::System
            && !(p
                .file_name()
                .is_some_and(|v| v.as_encoded_bytes().starts_with(b"ssystem"))
                && p.extension().is_some_and(|v| v == "dat"))
        {
            continue;
        }
        if paths.len() == limit {
            return Ok((paths, true));
        }
        paths.push(p);
    }
    Ok((paths, false))
}
/// Source references only select among already enumerated files. They never trigger reads.
fn reference_key(root: &Path, value: &[u8]) -> Option<String> {
    let referenced = Path::new(std::str::from_utf8(value).ok()?);
    if referenced.is_absolute() {
        return None;
    }
    let mut components = Vec::new();
    for c in referenced.components() {
        match c {
            Component::Normal(v) => components.push(v),
            Component::CurDir => {}
            Component::ParentDir => {
                components.pop()?;
            }
            _ => return None,
        }
    }
    if components.len() != 3 || components[0] != root.file_name()? || components[1] != "orbits" {
        return None;
    }
    Some(format!("orbits/{}", components[2].to_str()?))
}
impl Dataset {
    pub fn load(root: &Path, limits: Limits) -> Self {
        let mut dataset = Self::default();
        let (mut total_bytes, mut records, mut fields) = (0, 0, 0);
        for kind in [SourceKind::System, SourceKind::Orbit] {
            let folder = if kind == SourceKind::System {
                root.to_path_buf()
            } else {
                root.join("orbits")
            };
            let (mut paths, cut) =
                match discover(&folder, kind, limits.max_sources - dataset.sources.len()) {
                    Ok(v) => v,
                    Err(e) => {
                        dataset.diagnostics.push(issue(
                            &folder.to_string_lossy(),
                            0,
                            0,
                            e.to_string(),
                        ));
                        continue;
                    }
                };
            if cut {
                dataset.diagnostics.push(issue(
                    &folder.to_string_lossy(),
                    0,
                    0,
                    "source count limit reached",
                ));
            }
            paths.sort();
            if kind == SourceKind::System {
                for expected in ["ssystem.dat", "ssystem_jp.dat"] {
                    if !paths
                        .iter()
                        .any(|p| p.file_name().is_some_and(|n| n == expected))
                    {
                        dataset.diagnostics.push(issue(
                            expected,
                            0,
                            0,
                            "optional definition missing",
                        ));
                    }
                }
            }
            for path in paths {
                let key_name = path
                    .strip_prefix(root)
                    .expect("enumerated child")
                    .to_string_lossy()
                    .replace(std::path::MAIN_SEPARATOR, "/");
                let mut src = Source {
                    key: key_name.clone(),
                    kind,
                    raw: vec![],
                    read_complete: false,
                    parsed: false,
                    valid: false,
                    records: vec![],
                };
                let read = fs::symlink_metadata(&path).and_then(|m| {
                    if m.is_file() {
                        read_bounded(
                            &path,
                            limits
                                .catalog
                                .max_file_bytes
                                .min(limits.max_total_bytes - total_bytes),
                        )
                    } else {
                        Err(io::Error::new(
                            io::ErrorKind::InvalidInput,
                            "satellite source must be a direct regular file",
                        ))
                    }
                });
                match read {
                    Err(e) => dataset
                        .diagnostics
                        .push(issue(&key_name, 0, 0, e.to_string())),
                    Ok(raw) => {
                        total_bytes += raw.len() as u64;
                        src.raw = raw;
                        src.read_complete = true;
                        match parse(
                            &key_name,
                            kind,
                            &src.raw,
                            limits,
                            limits.catalog.max_records - records,
                            limits.max_fields - fields,
                        ) {
                            Err(e) => dataset.diagnostics.push(e),
                            Ok(p) => {
                                fields += p.fields;
                                records += p.records.len();
                                src.parsed = true;
                                src.valid = p.valid;
                                src.records = p.records;
                                dataset.diagnostics.extend(p.diagnostics);
                            }
                        }
                    }
                }
                dataset.sources.push(src);
            }
        }
        let orbits: BTreeMap<_, _> = dataset
            .sources
            .iter()
            .enumerate()
            .filter(|(_, s)| s.kind == SourceKind::Orbit && s.read_complete)
            .map(|(i, s)| (s.key.clone(), (i, !s.records.is_empty())))
            .collect();
        for source in dataset
            .sources
            .iter_mut()
            .filter(|s| s.kind == SourceKind::System)
        {
            for record in &mut source.records {
                if let Some(field) = record.field("Orbit") {
                    let target = reference_key(root, &field.value_bytes)
                        .and_then(|k| orbits.get(&k))
                        .copied();
                    if !target.is_some_and(|(_, parsed)| parsed) {
                        dataset.diagnostics.push(issue(
                            &source.key,
                            field.line,
                            field.byte_offset,
                            "no unique parsed orbit in the selected dataset",
                        ));
                    }
                    record.orbit_source = target.map(|(i, _)| i);
                }
            }
        }
        dataset
    }
}
