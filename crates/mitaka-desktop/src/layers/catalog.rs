use mitaka_core::{
    catalog::{CatalogLimits, Constellation},
    constellation_art::{self, Art},
    extended_catalog,
    place_catalog::{self, PlaceCatalog},
};
use std::{
    borrow::Cow,
    collections::{HashMap, HashSet},
    fs, io,
    path::Path,
};

pub struct ArtRecord {
    pub abbreviation: Vec<u8>,
    pub art: Art,
}
#[derive(Default)]
pub struct LayersCatalog {
    pub names: HashMap<i32, String>,
    pub featured: HashSet<i32>,
    pub hosts: HashSet<i32>,
    pub art: Vec<ArtRecord>,
    pub places: PlaceCatalog,
    pub warnings: Vec<String>,
}
pub fn decode(bytes: &[u8]) -> Cow<'_, str> {
    std::str::from_utf8(bytes)
        .map(Cow::Borrowed)
        .unwrap_or_else(|_| encoding_rs::SHIFT_JIS.decode_without_bom_handling(bytes).0)
}
impl LayersCatalog {
    pub fn load(root: &Path, constellations: &[Constellation]) -> Self {
        let mut out = Self::default();
        match extended_catalog::load_star_names(&root.join("star_names_HIP.dat")) {
            Ok(rows) => {
                for row in rows {
                    let name = if matches!(row.localized_name_bytes.as_slice(), b"" | b"*") {
                        &row.name_bytes
                    } else {
                        &row.localized_name_bytes
                    };
                    if !matches!(name.as_slice(), b"" | b"*") {
                        out.names.insert(row.hip, decode(name).into_owned());
                    }
                    if row.featured {
                        out.featured.insert(row.hip);
                    }
                }
            }
            Err(e) if e.kind() != io::ErrorKind::NotFound => out.warnings.push(e.to_string()),
            _ => {}
        }
        match extended_catalog::load_exoplanet_hosts(&root.join("exoplanets_HIP_list.dat")) {
            Ok(rows) => out.hosts.extend(rows),
            Err(e) if e.kind() != io::ErrorKind::NotFound => out.warnings.push(e.to_string()),
            _ => {}
        }
        match place_catalog::load_observer_places(root) {
            Ok(c) => out.places = c,
            Err(e) => out.warnings.push(e.to_string()),
        }
        if let Err(e) = out.load_art(&root.join("constellations"), constellations) {
            if e.kind() != io::ErrorKind::NotFound {
                out.warnings.push(e.to_string());
            }
        }
        out
    }
    fn load_art(&mut self, root: &Path, records: &[Constellation]) -> io::Result<()> {
        let limits = CatalogLimits::default();
        let mut paths = vec![];
        let mut total = 0u64;
        for entry in fs::read_dir(root)? {
            let entry = entry?;
            let path = entry.path();
            if entry.file_type()?.is_file()
                && path
                    .extension()
                    .and_then(|x| x.to_str())
                    .is_some_and(|x| x.eq_ignore_ascii_case("cab"))
            {
                if paths.len() >= limits.max_records {
                    return Err(io::Error::other("CAB file count limit"));
                }
                paths.push(path);
            }
        }
        paths.sort();
        for path in paths {
            let bytes = path.metadata()?.len();
            total = total.saturating_add(bytes);
            if total > limits.max_file_bytes {
                return Err(io::Error::other("CAB cumulative byte limit"));
            }
            let stem = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            let name = if matches!(stem.as_str(), "serh" | "sert") {
                "ser"
            } else {
                &stem
            };
            let Some(record) = records
                .iter()
                .find(|c| c.abbreviation.eq_ignore_ascii_case(name.as_bytes()))
            else {
                self.warnings
                    .push(format!("{}: 星座略号に対応しません", path.display()));
                continue;
            };
            match constellation_art::load(
                &path,
                CatalogLimits {
                    max_file_bytes: bytes,
                    ..limits
                },
            ) {
                Ok(art) => self.art.push(ArtRecord {
                    abbreviation: record.abbreviation.clone(),
                    art,
                }),
                Err(e) => self.warnings.push(e.to_string()),
            }
        }
        Ok(())
    }
}
