//! グループ別の表示状態。小天体・衛星の元ファイルはブラウザと描画で共有する。
use super::{catalog::Catalog, layers::Options, Kind};
use eframe::egui;
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    io,
    path::{Path, PathBuf},
    rc::Rc,
    sync::mpsc,
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Sampling {
    pub first: f64,
    pub last: f64,
    pub count: usize,
}
impl Default for Sampling {
    fn default() -> Self {
        Self {
            first: 2451362.375,
            last: 2451727.625,
            count: 201,
        }
    }
}
#[derive(Default)]
pub(crate) struct Display {
    pub small: bool,
    pub craft: bool,
    pub(super) groups: [Options; 5],
    pub(super) sampling: Sampling,
}
const TITLES: [&str; 5] = ["小惑星", "太陽系外縁天体", "彗星", "自然衛星", "探査機"];
const HOST: [&str; 4] = ["asteroids", "tnobjects", "comets", "satellites"];
impl Display {
    pub(super) fn for_row(&self, catalog: &Catalog, i: usize) -> Option<&Options> {
        let group = catalog.group(i)?;
        if group < 3 && !self.small || group == 4 && !self.craft {
            return None;
        }
        Some(&self.groups[group])
    }
    pub(super) fn group_visible(&self, kind: Kind) -> bool {
        let any = |o: &Options| o.points || o.labels || o.paths;
        match kind {
            Kind::Small => self.small && self.groups[..3].iter().any(any),
            Kind::Satellites => any(&self.groups[3]),
            Kind::Spacecraft => self.craft && any(&self.groups[4]),
            _ => false,
        }
    }
    pub fn ui(&mut self, ui: &mut egui::Ui) {
        ui.collapsing("小天体・衛星・探査機", |ui| {
            ui.checkbox(&mut self.small, "小天体レイヤー");
            ui.checkbox(&mut self.craft, "探査機レイヤー");
            egui::Grid::new("object-display-flags").show(ui, |ui| {
                for label in ["種類", "点", "名称", "軌道", "選択のみ"] {
                    ui.label(label);
                }
                ui.end_row();
                for (i, (options, label)) in self.groups.iter_mut().zip(TITLES).enumerate() {
                    ui.label(label);
                    ui.checkbox(&mut options.points, "");
                    ui.checkbox(&mut options.labels, "");
                    ui.checkbox(&mut options.paths, "");
                    if i < 4 {
                        ui.checkbox(&mut options.selected_only, "");
                    }
                    ui.end_row();
                }
            });
            ui.label(format!(
                "彗星の軌跡  JD {} ～ {}（{}点）",
                self.sampling.first, self.sampling.last, self.sampling.count
            ));
            let mut next = self.sampling;
            ui.horizontal(|ui| {
                ui.add(egui::DragValue::new(&mut next.first).prefix("開始 JD "));
                ui.add(egui::DragValue::new(&mut next.last).prefix("終了 JD "));
                ui.add(
                    egui::DragValue::new(&mut next.count)
                        .range(2..=4096)
                        .suffix(" 点"),
                );
            });
            if next.last > next.first {
                self.sampling = next;
            }
        });
    }
    pub fn snapshot(&self, out: &mut Value) {
        for (name, o) in HOST.iter().zip(&self.groups) {
            for (k, v) in [
                ("visible", o.points),
                ("labels", o.labels),
                ("orbits", o.paths),
                ("selectedOnly", o.selected_only),
            ] {
                out[format!("{name}_{k}")] = v.into();
            }
        }
    }
    pub fn apply(&mut self, kind: u64, flag: bool) {
        let index = (kind - 44) as usize;
        let o = &mut self.groups[index / 4];
        match index % 4 {
            0 => o.points = flag,
            1 => o.labels = flag,
            2 => o.paths = flag,
            _ => o.selected_only = flag,
        }
    }
    pub fn store(&self, v: &mut Value) {
        let encode = |o: &Options| json!({"visible":o.points,"labels":o.labels,"orbits":o.paths,"selected_only":o.selected_only});
        v["small_bodies"]["visible"] = self.small.into();
        v["small_bodies"]["kinds"] = Value::Array(self.groups[..3].iter().map(encode).collect());
        v["small_bodies"]["orbit_sampling"] = json!({"first_jd":self.sampling.first,"last_jd":self.sampling.last,"sample_count":self.sampling.count});
        v["satellites"]["options"] = encode(&self.groups[3]);
        let c = self.groups[4];
        v["spacecraft"] =
            json!({"visible":self.craft,"points":c.points,"labels":c.labels,"orbits":c.paths});
    }
    pub fn restore(v: &Value) -> Self {
        let small = &v["small_bodies"];
        let sampling = &small["orbit_sampling"];
        let mut groups = [Options::default(); 5];
        for (i, o) in groups.iter_mut().enumerate() {
            let record = match i {
                0..=2 => &small["kinds"][i],
                3 => &v["satellites"]["options"],
                _ => &v["spacecraft"],
            };
            o.points = record[if i == 4 { "points" } else { "visible" }] == true;
            o.paths = record["orbits"] == true;
            o.labels = record["labels"] == true;
            o.selected_only = record["selected_only"] == true;
        }
        Self {
            small: small["visible"] == true,
            craft: v["spacecraft"]["visible"] == true,
            groups,
            sampling: Sampling {
                first: sampling["first_jd"].as_f64().unwrap(),
                last: sampling["last_jd"].as_f64().unwrap(),
                count: sampling["sample_count"].as_f64().unwrap() as usize,
            },
        }
    }
}
enum Entry {
    Pending(mpsc::Receiver<io::Result<Catalog>>),
    Ready(Rc<Catalog>),
    Error(String),
}
#[derive(Default)]
pub(super) struct Catalogs {
    root: PathBuf,
    entries: HashMap<Kind, Entry>,
}
impl Catalogs {
    fn ensure_root(&mut self, root: &Path) {
        if root != self.root {
            self.entries.clear();
            self.root = root.to_owned()
        }
    }
    pub fn insert(&mut self, root: &Path, kind: Kind, c: Rc<Catalog>) {
        self.ensure_root(root);
        self.entries.insert(kind, Entry::Ready(c));
    }
    pub fn get(
        &mut self,
        kind: Kind,
        sky: &crate::catalog::SkyData,
        ctx: &egui::Context,
    ) -> Result<Option<Rc<Catalog>>, String> {
        let root = &sky.root;
        self.ensure_root(root);
        let entry = self.entries.entry(kind).or_insert_with(|| {
            let (tx, rx) = mpsc::channel();
            let root = root.to_owned();
            let ctx = ctx.clone();
            let stars = (kind == Kind::Stars).then(|| sky.stars.clone());
            std::thread::spawn(move || {
                let _ = tx.send(Catalog::load_with_stars(kind, &root, stars.as_deref()));
                ctx.request_repaint();
            });
            Entry::Pending(rx)
        });
        if let Entry::Pending(rx) = entry {
            match rx.try_recv() {
                Ok(Ok(c)) => *entry = Entry::Ready(Rc::new(c)),
                Ok(Err(e)) => *entry = Entry::Error(e.to_string()),
                Err(mpsc::TryRecvError::Empty) => return Ok(None),
                Err(_) => *entry = Entry::Error("カタログ読込スレッドが終了しました".into()),
            }
        }
        match entry {
            Entry::Ready(c) => Ok(Some(c.clone())),
            Entry::Error(e) => Err(e.clone()),
            Entry::Pending(_) => unreachable!(),
        }
    }
}
