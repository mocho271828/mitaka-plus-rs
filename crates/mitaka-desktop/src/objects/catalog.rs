use super::{Kind, Position};
use crate::catalog::SkyData;
use mitaka_core::{
    catalog::{load_constellations, load_hipparcos},
    extended_catalog::{load_ngc, load_sdss, load_star_names, SdssKind, SdssRecord},
    small_body,
};
use std::{collections::HashMap, io, path::Path, sync::Arc};
use unicode_normalization::UnicodeNormalization;

pub(super) fn name(bytes: &[u8]) -> String {
    let bytes = bytes.trim_ascii();
    let text = std::str::from_utf8(bytes)
        .map(std::borrow::Cow::Borrowed)
        .unwrap_or_else(|_| encoding_rs::SHIFT_JIS.decode_without_bom_handling(bytes).0);
    text.trim_matches(['\0', ' ']).to_owned()
}
pub(super) fn error(message: &str) -> io::Error {
    io::Error::other(message)
}

struct Entry {
    label: String,
    source: String,
    key: String,
    target: Key,
}
impl Entry {
    fn new(label: String, source: String, target: Key) -> Self {
        let key = match &target {
            Key::Hip(_) | Key::Planet(_) | Key::Nebula(_) | Key::Sky(_) => normalize(&label),
            _ => normalize(&format!("{label} {source}")),
        };
        Self {
            label,
            source,
            key,
            target,
        }
    }
}
fn normalize(s: &str) -> String {
    s.nfkd()
        .filter(|c| !unicode_normalization::char::is_combining_mark(*c))
        .filter(|c| !c.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect()
}

enum Key {
    Hip(i32),
    Planet(usize),
    Sky([f64; 2]),
    Nebula(mitaka_core::extended_catalog::NgcRecord),
    Small(usize, usize),
    Satellite(usize, usize),
    Craft(usize),
}
enum Store {
    Static,
    Small(small_body::Dataset),
    Satellites(super::satellite::Satellites),
    Craft(super::spacecraft::Craft),
    Sdss(&'static str, Vec<SdssRecord>),
}
pub(super) struct Catalog {
    pub(super) sources: std::collections::BTreeMap<usize, super::selections::Source>,
    rows: Arc<[Entry]>,
    store: Store,
    pub warnings: Vec<String>,
    paths: std::cell::RefCell<PathCache>,
    positions: std::cell::RefCell<Positions>,
}
type OrbitPath = std::sync::Arc<[Option<[f64; 3]>]>;
#[derive(Default)]
struct PathCache {
    sampling: super::display::Sampling,
    paths: std::collections::HashMap<usize, Option<OrbitPath>>,
}
#[derive(Default)]
struct Positions {
    jd: Option<f64>,
    positions: std::collections::HashMap<usize, Option<[f64; 3]>>,
}
pub(super) struct SearchRows {
    pub kind: Kind,
    rows: Arc<[Entry]>,
    pub total: usize,
}
impl SearchRows {
    pub(super) fn words(query: &str) -> Vec<String> {
        let mut words = Vec::new();
        for word in query
            .split_whitespace()
            .map(normalize)
            .filter(|s| !s.is_empty())
        {
            let prefix = ["hip", "ngc", "ic"].into_iter().find(|p| {
                word.strip_prefix(p)
                    .is_some_and(|s| s.as_bytes().first().is_some_and(u8::is_ascii_digit))
            });
            if let Some(prefix) = prefix {
                words.push(prefix.to_owned());
                words.push(word[prefix.len()..].to_owned());
            } else {
                words.push(word);
            }
        }
        words
    }

    pub(super) fn remaining_words<'a>(&self, words: &'a [String]) -> Option<Vec<&'a str>> {
        let group = match self.kind {
            Kind::Stars => "hip 恒星 star",
            Kind::Planets => "太陽系 planet",
            Kind::Constellations => "星座 constellation",
            Kind::Ngc => "星雲 星団 nebula",
            Kind::Galaxies => "sdssgal sdss_gal.dat 銀河 galaxy",
            Kind::Quasars => "sdssqso sdss_qso.dat クエーサー quasar",
            _ => "",
        };
        let mut remaining = Vec::new();
        for word in words {
            if group.contains(word) {
                continue;
            }
            if matches!(self.kind, Kind::Galaxies | Kind::Quasars)
                && !word
                    .trim_start_matches('#')
                    .bytes()
                    .all(|b| b.is_ascii_digit())
            {
                return None;
            }
            remaining.push(word.as_str());
        }
        Some(remaining)
    }
    pub(super) fn matches(&self, index: usize, words: &[&str]) -> bool {
        if let Some(row) = self.rows.get(index) {
            return words.iter().all(|&w| row.key.contains(w));
        }
        // SDSSの検索対象は元ファイルでの通し番号。識別子を全件分常駐させない。
        let numbered = (index + 1).to_string();
        words.iter().all(|&w| {
            w.strip_prefix('#')
                .map_or_else(|| numbered.contains(w), |n| n == numbered)
        })
    }
}
impl Catalog {
    pub(super) fn load(kind: Kind, root: &Path) -> io::Result<Self> {
        Self::load_with_stars(kind, root, None)
    }
    pub(super) fn load_with_stars(
        kind: Kind,
        root: &Path,
        stars: Option<&[mitaka_core::catalog::Star]>,
    ) -> io::Result<Self> {
        let mut sources = std::collections::BTreeMap::new();
        let mut rows = Vec::new();
        let mut warnings = Vec::new();
        let store = match kind {
            Kind::Planets => {
                for (id, english) in [
                    "Sun", "Mercury", "Venus", "Earth", "Mars", "Jupiter", "Saturn", "Uranus",
                    "Neptune", "Pluto", "Moon",
                ]
                .into_iter()
                .enumerate()
                {
                    rows.push(Entry::new(
                        format!("{} · {english}", crate::solar::NAMES[id]),
                        "太陽系 planet".into(),
                        Key::Planet(id),
                    ));
                }
                Store::Static
            }
            Kind::Stars => {
                let names = match load_star_names(&root.join("star_names_HIP.dat")) {
                    Ok(names) => names
                        .into_iter()
                        .map(|s| (s.hip, s))
                        .collect::<HashMap<_, _>>(),
                    Err(e) => {
                        if e.kind() != io::ErrorKind::NotFound {
                            warnings.push(e.to_string());
                        }
                        HashMap::new()
                    }
                };
                let on_disk;
                let stars = match stars {
                    Some(stars) => stars,
                    None => {
                        on_disk = load_hipparcos(&root.join("hip.dat"))?;
                        &on_disk
                    }
                };
                for star in stars {
                    let mut label = format!("HIP {}", star.hip);
                    if let Some(s) = names.get(&star.hip) {
                        for alias in [&s.name_bytes, &s.localized_name_bytes] {
                            let alias = name(alias);
                            if !alias.is_empty() && alias != "*" {
                                label.push_str(" · ");
                                label.push_str(&alias);
                            }
                        }
                    }
                    rows.push(Entry::new(
                        label,
                        format!(
                            "hip.dat · 記録 {} · V {:.2}",
                            star.source_row, star.magnitude
                        ),
                        Key::Hip(star.hip),
                    ));
                }
                Store::Static
            }
            Kind::Constellations => {
                let file = "constellation_HIP.dat";
                for c in load_constellations(&root.join(file))? {
                    let names = [
                        c.abbreviation,
                        c.english_name,
                        c.english_genitive,
                        c.japanese_kanji_bytes,
                        c.japanese_hiragana_bytes,
                    ]
                    .map(|s| name(&s));
                    rows.push(Entry::new(
                        names.join("　"),
                        format!("{file} · 記録 {}", c.source_row),
                        Key::Sky([c.ra_deg.into(), c.dec_deg.into()]),
                    ));
                }
                Store::Static
            }
            Kind::Ngc => {
                let file = "ngc.dat";
                for (i, n) in load_ngc(&root.join(file))?.into_iter().enumerate() {
                    let prefix = if n.is_ngc { "NGC" } else { "IC" };
                    rows.push(Entry::new(
                        format!("{prefix} {}", n.number),
                        format!("{file} · 記録 {}", i + 1),
                        Key::Nebula(n),
                    ));
                }
                Store::Static
            }
            Kind::Galaxies | Kind::Quasars => {
                let (file, typ) = if kind == Kind::Galaxies {
                    ("SDSS_GAL.dat", SdssKind::Galaxy)
                } else {
                    ("SDSS_QSO.dat", SdssKind::Quasar)
                };
                Store::Sdss(file, load_sdss(&root.join(file), typ)?)
            }
            Kind::Small => {
                let dataset = small_body::Dataset::load(root, Default::default());
                for (s, src) in dataset.sources.iter().enumerate() {
                    if let Some(e) = &src.error {
                        warnings.push(format!("{} · {e}", src.key));
                    }
                    if !src.records.is_empty() && src.error.is_none() {
                        sources.insert(s, super::selections::Source::new(&src.key, &src.raw));
                    }
                    for (i, r) in src.records.iter().enumerate() {
                        let mut parts = Vec::new();
                        if let Some(id) = r.number {
                            parts.push(format!("({id})"));
                        }
                        for bytes in [&r.name_bytes, &r.display_name_bytes]
                            .into_iter()
                            .chain(&r.aliases)
                        {
                            let t = name(bytes);
                            if !t.is_empty() && !parts.contains(&t) {
                                parts.push(t);
                            }
                        }
                        let label = if parts.is_empty() {
                            format!("{} #{}", src.key, i + 1)
                        } else {
                            parts.join("　")
                        };
                        rows.push(Entry::new(
                            label,
                            format!("{} · 記録 {} · byte {}", src.key, i + 1, r.byte_range.start),
                            Key::Small(s, i),
                        ));
                    }
                }
                warnings.extend(
                    dataset
                        .directory_errors
                        .iter()
                        .map(|(p, e)| format!("{p} · {e}")),
                );
                Store::Small(dataset)
            }
            Kind::Satellites => {
                let dataset =
                    mitaka_core::satellite_catalog::Dataset::load(root, Default::default());
                warnings.extend(
                    dataset
                        .diagnostics
                        .iter()
                        .map(|d| format!("{} 行 {} · {}", d.source, d.line, d.message)),
                );
                for (s, src) in dataset.sources.iter().enumerate() {
                    if src.read_complete && src.records.iter().any(|r| r.block == "Satellite") {
                        sources.insert(s, super::selections::Source::new(&src.key, &src.raw));
                    }
                    for r in src.records.iter().filter(|r| r.block == "Satellite") {
                        let parent = r
                            .parent
                            .and_then(|p| src.records.get(p))
                            .and_then(|p| p.name())
                            .map(name)
                            .unwrap_or_default();
                        let title = r
                            .field("Name")
                            .map(|f| name(&f.value_bytes))
                            .unwrap_or_else(|| "#?".into());
                        rows.push(Entry::new(
                            format!("{title}　({parent})"),
                            format!("{} · 行 {}", src.key, r.line),
                            Key::Satellite(s, r.ordinal),
                        ));
                    }
                }
                Store::Satellites(super::satellite::Satellites::prepare(
                    &dataset,
                    &mut warnings,
                ))
            }
            Kind::Spacecraft => {
                let craft = super::spacecraft::Craft::load(root);
                warnings.extend(craft.warnings());
                for (i, title, source) in craft.entries() {
                    rows.push(Entry::new(title, source, Key::Craft(i)));
                }
                Store::Craft(craft)
            }
        };
        Ok(Self {
            sources,
            rows: rows.into(),
            store,
            warnings,
            paths: Default::default(),
            positions: Default::default(),
        })
    }
    pub(super) fn search_rows(&self, kind: Kind) -> SearchRows {
        SearchRows {
            kind,
            rows: self.rows.clone(),
            total: self.len(),
        }
    }
    pub(super) fn group(&self, i: usize) -> Option<usize> {
        match (&self.store, &self.rows.get(i)?.target) {
            (Store::Small(d), Key::Small(s, r)) => Some(match d.sources[*s].records[*r].kind {
                small_body::Kind::Asteroid => 0,
                small_body::Kind::TransNeptunian => 1,
                small_body::Kind::Comet => 2,
            }),
            (Store::Satellites(_), Key::Satellite(..)) => Some(3),
            (Store::Craft(_), Key::Craft(_)) => Some(4),
            _ => None,
        }
    }
    pub(super) fn scene_point(&self, i: usize, sky: &SkyData, jd: f64) -> Option<[f64; 3]> {
        let mut cache = self.positions.borrow_mut();
        if cache.jd != Some(jd) {
            cache.positions.clear();
            cache.jd = Some(jd);
        }
        *cache
            .positions
            .entry(i)
            .or_insert_with(|| match self.locate(i, sky, jd) {
                Ok(Position::Solar(p, _)) => Some(p),
                _ => None,
            })
    }
    pub(super) fn identity(&self, i: usize) -> Option<(&super::selections::Source, usize)> {
        let (s, ord) = match self.rows.get(i)?.target {
            Key::Small(s, ord) | Key::Satellite(s, ord) => (s, ord),
            _ => return None,
        };
        Some((self.sources.get(&s)?, ord))
    }
    pub(super) fn len(&self) -> usize {
        if let Store::Sdss(_, rows) = &self.store {
            rows.len()
        } else {
            self.rows.len()
        }
    }
    pub(super) fn title(&self, i: usize) -> String {
        if let Store::Sdss(file, _) = &self.store {
            format!("{file} #{}", i + 1)
        } else {
            self.rows[i].label.clone()
        }
    }
    pub(super) fn nebula_magnitude(&self, i: usize) -> Option<f32> {
        match self.rows.get(i)?.target {
            Key::Nebula(r) => Some(r.magnitude),
            _ => None,
        }
    }
    pub(super) fn info(&self, i: usize, jd: f64) -> String {
        if let Store::Sdss(file, rows) = &self.store {
            let r = &rows[i];
            format!(
                "{file} · 記録 {} · 赤経 {:.5}°　赤緯 {:+.5}°　距離 {:.5} pc　{} {:.5}",
                i + 1,
                r.ra_deg,
                r.dec_deg,
                r.distance_pc,
                if *file == "SDSS_GAL.dat" {
                    "eClass"
                } else {
                    "z"
                },
                r.classification_value
            )
        } else {
            let r = &self.rows[i];
            let detail = match (&self.store, &r.target) {
                (_, Key::Nebula(n)) => {
                    let mag = if n.magnitude == 0. {
                        String::from("等級未記載")
                    } else {
                        format!(
                            "{} 等級 {:.2}",
                            if n.photographic_magnitude {
                                "写真"
                            } else {
                                "V"
                            },
                            n.magnitude
                        )
                    };
                    format!(
                        "赤経 {:.5}°　赤緯 {:+.5}°　{mag}　視直径 {:.2}′",
                        n.ra_deg, n.dec_deg, n.size_arcmin
                    )
                }
                (Store::Craft(c), Key::Craft(i)) => c.sampling(*i, jd).unwrap_or_default(),
                (Store::Small(d), Key::Small(s, i)) => {
                    let body = &d.sources[*s].records[*i];
                    format!(
                        "{:?} · {:?} · {} bytes",
                        body.kind,
                        body.encoding,
                        body.byte_range.len()
                    )
                }
                _ => String::new(),
            };
            if detail.is_empty() {
                r.source.clone()
            } else {
                format!("{}\n{detail}", r.source)
            }
        }
    }
    // SDSSは原版に固有名がなく、原ファイル内の1始まりの記録番号で検索する。
    pub(super) fn search(&self, query: &str) -> Option<Vec<usize>> {
        let q = normalize(query);
        if q.is_empty() {
            return None;
        }
        if let Store::Sdss(file, rows) = &self.store {
            let label = normalize(file);
            let raw = q.strip_prefix(&label).unwrap_or(&q).trim_start_matches('#');
            return Some(
                raw.parse::<usize>()
                    .ok()
                    .filter(|i| (1..=rows.len()).contains(i))
                    .map(|i| i - 1)
                    .into_iter()
                    .collect(),
            );
        }
        Some(
            self.rows
                .iter()
                .enumerate()
                .filter(|(_, e)| e.key.contains(&q))
                .map(|(i, _)| i)
                .collect(),
        )
    }
    pub(super) fn satellite_path(
        &self,
        i: usize,
        system: &mitaka_core::ephemeris::Cf96Ephemeris,
        jd: f64,
    ) -> Option<std::sync::Arc<[[f64; 3]]>> {
        match (&self.store, &self.rows.get(i)?.target) {
            (Store::Satellites(sat), Key::Satellite(source, ordinal)) => {
                sat.path(*source, *ordinal, system, jd).ok()
            }
            _ => None,
        }
    }
    pub(super) fn craft_path(&self, i: usize) -> Option<&[mitaka_core::spacecraft::Sample]> {
        match (&self.store, &self.rows.get(i)?.target) {
            (Store::Craft(c), Key::Craft(id)) => Some(c.trajectory(*id).ok()?.samples()),
            _ => None,
        }
    }
    pub(super) fn small_path(
        &self,
        i: usize,
        sampling: super::display::Sampling,
    ) -> Option<OrbitPath> {
        let mut cache = self.paths.borrow_mut();
        if cache.sampling != sampling {
            cache.paths.clear();
            cache.sampling = sampling;
        }
        cache
            .paths
            .entry(i)
            .or_insert_with(|| self.build_small_path(i, sampling))
            .clone()
    }
    fn build_small_path(&self, i: usize, sampling: super::display::Sampling) -> Option<OrbitPath> {
        let (Store::Small(d), Key::Small(src, r)) = (&self.store, &self.rows.get(i)?.target) else {
            return None;
        };
        let record = &d.sources[*src].records[*r];
        let dates = match record.orbit {
            small_body::Orbit::Mean(e) if e.eccentricity < 1. => {
                let phase = e.mean_anomaly_deg.rem_euclid(360.);
                let period = 360. / e.mean_motion_deg_per_day;
                let first = e.epoch_jd - phase / e.mean_motion_deg_per_day;
                (0..sampling.count)
                    .map(|i| {
                        let angle = std::f64::consts::TAU * i as f64 / (sampling.count - 1) as f64;
                        first
                            + period * (angle - e.eccentricity * angle.sin())
                                / std::f64::consts::TAU
                    })
                    .collect::<Vec<_>>()
            }
            small_body::Orbit::Perihelion(_) => (0..sampling.count)
                .map(|i| {
                    sampling.first
                        + (sampling.last - sampling.first) * i as f64 / (sampling.count - 1) as f64
                })
                .collect(),
            _ => return None,
        };
        if dates.windows(2).any(|p| !p[0].is_finite() || p[1] <= p[0]) {
            return None;
        }
        Some(dates.into_iter().map(|t| record.position(t).ok()).collect())
    }
    pub(super) fn satellite_source(&self, row: usize) -> Option<usize> {
        if let Key::Satellite(source, _) = self.rows.get(row)?.target {
            Some(source)
        } else {
            None
        }
    }
    pub(super) fn script_row(
        &self,
        prefix: &str,
        name: &str,
        satellite_source: Option<usize>,
    ) -> io::Result<usize> {
        let mut matches =
            self.rows
                .iter()
                .enumerate()
                .filter(|(_, row)| match (&self.store, &row.target) {
                    (Store::Small(d), Key::Small(s, i)) => {
                        let r = &d.sources[*s].records[*i];
                        let expected = match prefix {
                            "Asteroids" => small_body::Kind::Asteroid,
                            "TNO" => small_body::Kind::TransNeptunian,
                            _ => small_body::Kind::Comet,
                        };
                        r.kind == expected
                            && (name
                                .parse::<i32>()
                                .ok()
                                .is_some_and(|n| n > 0 && r.number == Some(n))
                                || r.name_bytes == name.as_bytes()
                                || r.aliases.iter().any(|a| a == name.as_bytes()))
                    }
                    (Store::Craft(c), Key::Craft(i)) => c.match_name(*i, name),
                    (Store::Satellites(_), Key::Satellite(s, _)) => {
                        satellite_source.is_none_or(|selected| selected == *s)
                            && row.label.split_whitespace().next() == Some(name)
                    }
                    _ => false,
                });
        let (i, _) = matches
            .next()
            .ok_or_else(|| error("天体名が見つかりません"))?;
        if matches.next().is_some() {
            return Err(error("複数の原記録がこの天体名に一致します"));
        }
        Ok(i)
    }
    pub(super) fn locate(&self, i: usize, sky: &SkyData, jd: f64) -> io::Result<Position> {
        if let Store::Sdss(_, rows) = &self.store {
            let r = rows.get(i).ok_or_else(|| error("記録が見つかりません"))?;
            return Position::equatorial([r.ra_deg.into(), r.dec_deg.into()], None);
        }
        let row = self
            .rows
            .get(i)
            .ok_or_else(|| error("記録が見つかりません"))?;
        match (&self.store, &row.target) {
            (_, Key::Planet(id)) => Ok(Position::Planet(*id)),
            (_, Key::Hip(hip)) => {
                let s = sky
                    .by_hip
                    .get(hip)
                    .and_then(|r| sky.stars.get(*r))
                    .ok_or_else(|| error("HIP カタログに対応する恒星がありません"))?;
                Position::equatorial([s.ra_deg.into(), s.dec_deg.into()], Some(*hip))
            }
            (_, Key::Nebula(n)) => Position::equatorial([n.ra_deg.into(), n.dec_deg.into()], None),
            (_, Key::Sky(pos)) => Position::equatorial(*pos, None),
            (Store::Small(set), Key::Small(s, r)) => {
                Position::solar(set.sources[*s].records[*r].position(jd)?, 0.10)
            }
            (Store::Satellites(set), Key::Satellite(s, r)) => {
                Position::solar(set.position(*s, *r, &sky.ephemeris, jd)?, 0.001)
            }
            (Store::Craft(c), Key::Craft(i)) => c.position(*i, jd),
            _ => Err(error("天体の位置参照が一致しません")),
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sdss_identifiers_keep_source_ordinals_and_never_wrap() {
        let c = Catalog {
            paths: Default::default(),
            positions: Default::default(),
            rows: Arc::default(),
            sources: Default::default(),
            store: Store::Sdss(
                "SDSS_GAL.dat",
                vec![SdssRecord {
                    ra_deg: 1.,
                    dec_deg: 2.,
                    distance_pc: 1.,
                    classification_value: 0.,
                }],
            ),
            warnings: vec![],
        };
        assert_eq!(c.search("SDSS_GAL.dat #1"), Some(vec![0]));
        assert_eq!(c.search("#0"), Some(vec![]));
        assert_eq!(c.search("2"), Some(vec![]));
        assert_eq!(c.search(""), None);
        assert_eq!(
            name(&[0x83, 0x56, 0x83, 0x8a, 0x83, 0x45, 0x83, 0x58]),
            "シリウス"
        );
    }
}
