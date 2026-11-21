use super::{Kind, Position};
use crate::catalog::SkyData;
use mitaka_core::{
    catalog::load_constellations,
    extended_catalog::{load_ngc, load_sdss, load_star_names, SdssKind, SdssRecord},
    small_body,
};
use std::{io, path::Path};

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
        let key = normalize(&format!("{label} {source}"));
        Self {
            label,
            source,
            key,
            target,
        }
    }
}
fn normalize(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect()
}

enum Key {
    Hip(i32),
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
    rows: Vec<Entry>,
    store: Store,
    pub warnings: Vec<String>,
}
impl Catalog {
    pub(super) fn load(kind: Kind, root: &Path) -> io::Result<Self> {
        let mut sources = std::collections::BTreeMap::new();
        let mut rows = Vec::new();
        let mut warnings = Vec::new();
        let store = match kind {
            Kind::Stars => {
                let file = "star_names_HIP.dat";
                for s in load_star_names(&root.join(file))? {
                    rows.push(Entry::new(
                        format!(
                            "{}　{}　HIP {}",
                            name(&s.name_bytes),
                            name(&s.localized_name_bytes),
                            s.hip
                        ),
                        format!("{file} · 行 {}", s.source_row),
                        Key::Hip(s.hip),
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
            rows,
            store,
            warnings,
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
    pub(super) fn craft_path(&self, i: usize) -> Option<&[mitaka_core::spacecraft::Sample]> {
        match (&self.store, &self.rows.get(i)?.target) {
            (Store::Craft(c), Key::Craft(id)) => Some(c.trajectory(*id).ok()?.samples()),
            _ => None,
        }
    }
    pub(super) fn small_path(&self, i: usize, jd: f64) -> Option<Vec<Option<[f64; 3]>>> {
        let (Store::Small(d), Key::Small(src, r)) = (&self.store, &self.rows.get(i)?.target) else {
            return None;
        };
        let record = &d.sources[*src].records[*r];
        let dates = match record.orbit {
            small_body::Orbit::Mean(e) if e.eccentricity < 1. => {
                let phase = e.mean_anomaly_deg.rem_euclid(360.);
                let period = 360. / e.mean_motion_deg_per_day;
                let first = e.epoch_jd - phase / e.mean_motion_deg_per_day;
                (0..=180)
                    .map(|i| {
                        let angle = std::f64::consts::TAU * f64::from(i) / 180.;
                        first
                            + period * (angle - e.eccentricity * angle.sin())
                                / std::f64::consts::TAU
                    })
                    .collect::<Vec<_>>()
            }
            small_body::Orbit::Perihelion(_) => {
                (0..=180).map(|i| jd + f64::from(i) - 90.).collect()
            }
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
            rows: vec![],
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
