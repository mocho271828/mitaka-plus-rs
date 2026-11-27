use super::{
    catalog::{error, name},
    Position,
};
use mitaka_core::spacecraft::{Dataset, Pth, Sampling, SourceData};
use std::{io, path::Path};

pub(super) struct Craft(Dataset);
impl Craft {
    pub(super) fn load(root: &Path) -> Self {
        Self(Dataset::load(root, Default::default()))
    }
    pub(super) fn warnings(&self) -> Vec<String> {
        let mut w = Vec::new();
        for s in &self.0.sources {
            if let Some(e) = &s.error {
                w.push(format!("{} · {e}", s.key));
            }
            w.extend(s.diagnostics.iter().map(|t| format!("{} · {t}", s.key)));
        }
        w.extend(
            self.0
                .directory_errors
                .iter()
                .map(|(p, e)| format!("{p} · {e}")),
        );
        w
    }
    pub(super) fn match_name(&self, id: usize, key: &str) -> bool {
        let Some(SourceData::Definition(d)) = self.0.sources[id].data.as_ref() else {
            return false;
        };
        [
            (b"Name".as_slice(), b"".as_slice()),
            (b"DisplayName", b""),
            (b"KEY", b""),
            (b"NAME", b"ja"),
            (b"NAME", b"default"),
        ]
        .iter()
        .any(|(field, section)| d.field(field, section) == Some(key.as_bytes()))
    }
    pub(super) fn entries(&self) -> impl Iterator<Item = (usize, String, String)> + '_ {
        self.0.sources.iter().enumerate().filter_map(|(i, src)| {
            let SourceData::Definition(d) = src.data.as_ref()? else {
                return None;
            };
            let mut title = d
                .field(b"DisplayName", b"")
                .or_else(|| d.field(b"Name", b""))
                .map(name)
                .unwrap_or_else(|| src.key.clone());
            if let Some(n) = d.field(b"Name", b"").map(name).filter(|n| *n != title) {
                title.push_str(&format!("　{n}"));
            }
            let source = d
                .trajectory
                .and_then(|t| self.0.sources.get(t))
                .map_or_else(|| src.key.clone(), |t| format!("{} · {}", src.key, t.key));
            Some((i, title, source))
        })
    }
    pub(super) fn trajectory(&self, id: usize) -> io::Result<&Pth> {
        let definition = self.0.sources.get(id).and_then(|s| s.data.as_ref());
        let Some(SourceData::Definition(d)) = definition else {
            return Err(error("探査機の定義がありません"));
        };
        if !d.valid {
            return Err(error("探査機の定義に検証エラーがあります"));
        }
        let t = d
            .trajectory
            .and_then(|i| self.0.sources.get(i))
            .and_then(|s| s.data.as_ref())
            .ok_or_else(|| error("対応する探査機の軌道がありません"))?;
        let SourceData::Pth(path) = t else {
            return Err(error("この軌道には日心 J2000 黄道座標を適用できません"));
        };
        Ok(path)
    }
    pub(super) fn sampling(&self, id: usize, jd: f64) -> io::Result<String> {
        let path = self.trajectory(id)?;
        let p = path.position(jd)?;
        let kind = match p.sampling {
            Sampling::Exact => "記録日時と一致",
            Sampling::Interpolated => "記録間の線形補間",
            Sampling::ClampedBefore => "開始前のため先頭位置を表示",
            Sampling::ClampedAfter => "終了後のため最終位置を表示",
        };
        Ok(format!(
            "{kind} · 記録 {}–{} / {}",
            p.first + 1,
            p.second + 1,
            path.samples().len()
        ))
    }
    pub(super) fn position(&self, id: usize, jd: f64) -> io::Result<Position> {
        Position::solar(self.trajectory(id)?.position(jd)?.position_au, 0.1)
    }
}
