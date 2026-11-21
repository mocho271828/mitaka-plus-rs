use super::{catalog::Catalog, Kind};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    io,
    path::Path,
};

#[derive(Clone, PartialEq, Eq)]
pub(super) struct Source {
    key: String,
    count: usize,
    digest: String,
}
impl Source {
    pub(super) fn new(key: &str, raw: &[u8]) -> Self {
        Self {
            key: key.to_owned(),
            count: raw.len(),
            digest: format!("{:x}", Sha256::digest(raw)),
        }
    }
}

#[derive(Default)]
struct Group(BTreeMap<String, (Source, BTreeSet<usize>)>);
impl Group {
    fn json(&self, layer: &mut Value) {
        let mut sources = Vec::new();
        let mut selected = Vec::new();
        for (src, ordinals) in self.0.values() {
            let key = STANDARD.encode(src.key.as_bytes());
            sources
                .push(json!({"source_key_base64":key,"byte_count":src.count,"sha256":src.digest}));
            for ordinal in ordinals {
                selected.push(json!({"source_key_base64":key,"record_ordinal":ordinal}));
            }
        }
        layer["sources"] = json!(sources);
        layer["selected_ids"] = json!(selected);
    }

    fn from_data(root: &Path, kind: Kind, layer: &Value) -> io::Result<Self> {
        let mut result = Self::default();
        // セッションの構造と指紋・IDの対応関係は Session::decode で検証済み。
        let ids = layer["selected_ids"]
            .as_array()
            .ok_or_else(|| io::Error::other("天体IDが配列ではありません"))?;
        if ids.is_empty() {
            return Ok(result);
        }
        let catalog = Catalog::load(kind, root)?;
        let mut wanted: BTreeMap<Vec<u8>, BTreeSet<usize>> = BTreeMap::new();
        let bytes = |v: &Value| {
            STANDARD
                .decode(v["source_key_base64"].as_str().unwrap_or_default())
                .map_err(io::Error::other)
        };
        for id in ids {
            let ordinal = id["record_ordinal"]
                .as_f64()
                .ok_or_else(|| io::Error::other("記録番号が不正です"))?
                as usize;
            wanted.entry(bytes(id)?).or_default().insert(ordinal);
        }
        for orig in layer["sources"].as_array().into_iter().flatten() {
            let key = bytes(orig)?;
            let src = catalog
                .sources
                .values()
                .find(|s| s.key.as_bytes() == key)
                .ok_or_else(|| io::Error::other("選択天体の元ファイルがありません"))?;
            if Some(src.count as f64) != orig["byte_count"].as_f64() || orig["sha256"] != src.digest
            {
                return Err(io::Error::other(format!(
                    "保存時と原本が異なります {}",
                    src.key
                )));
            }
        }
        for i in 0..catalog.len() {
            let Some((src, ord)) = catalog.identity(i) else {
                continue;
            };
            if wanted
                .get_mut(src.key.as_bytes())
                .is_some_and(|s| s.remove(&ord))
            {
                result.toggle(src, ord, true);
            }
        }
        if wanted.values().any(|s| !s.is_empty()) {
            return Err(io::Error::other("元ファイルに記録番号がありません"));
        }
        Ok(result)
    }
    fn toggle(&mut self, src: &Source, ord: usize, on: bool) {
        if on {
            self.0
                .entry(src.key.clone())
                .or_insert_with(|| (src.clone(), BTreeSet::new()))
                .1
                .insert(ord);
        } else if let Some((_, rows)) = self.0.get_mut(&src.key) {
            rows.remove(&ord);
            if rows.is_empty() {
                self.0.remove(&src.key);
            }
        }
    }
}
#[derive(Default)]
pub(crate) struct Selections {
    small: Group,
    satellites: Group,
}
impl Selections {
    fn group_mut(&mut self, kind: Kind) -> Option<&mut Group> {
        match kind {
            Kind::Small => Some(&mut self.small),
            Kind::Satellites => Some(&mut self.satellites),
            _ => None,
        }
    }
    pub(super) fn contains(&self, kind: Kind, src: &Source, ord: usize) -> bool {
        let group = match kind {
            Kind::Small => &self.small,
            Kind::Satellites => &self.satellites,
            _ => return false,
        };
        group
            .0
            .get(&src.key)
            .is_some_and(|(file, rows)| file == src && rows.contains(&ord))
    }
    pub(super) fn toggle(&mut self, kind: Kind, src: &Source, ord: usize, on: bool) {
        if let Some(g) = self.group_mut(kind) {
            g.toggle(src, ord, on)
        }
    }
    pub(crate) fn apply_to(&self, state: &mut Value) {
        self.small.json(&mut state["small_bodies"]);
        self.satellites.json(&mut state["satellites"]);
    }
    pub(crate) fn load(root: &Path, state: &Value) -> io::Result<Self> {
        Ok(Self {
            small: Group::from_data(root, Kind::Small, &state["small_bodies"])?,
            satellites: Group::from_data(root, Kind::Satellites, &state["satellites"])?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "実カタログの元バイトの照合を行う"]
    fn source_fingerprints_restore_only_exact_original_records() {
        let dir = std::path::PathBuf::from(std::env::var_os("MITAKA_DATA_ROOT").unwrap());
        let mut selection = Selections::default();
        let mut state = crate::session::Session::default().0;
        for kind in [Kind::Small, Kind::Satellites] {
            let cat = Catalog::load(kind, &dir).unwrap();
            let (src, i) = cat.identity(0).unwrap();
            selection.toggle(kind, src, i, true);
        }
        selection.apply_to(&mut state);
        let got = Selections::load(&dir, &state).unwrap();
        let mut reencoded = crate::session::Session::default().0;
        got.apply_to(&mut reencoded);
        assert_eq!(state, reencoded);
        state["small_bodies"]["sources"][0]["sha256"] = json!("f".repeat(64));
        assert!(Selections::load(&dir, &state).is_err());
    }
}
