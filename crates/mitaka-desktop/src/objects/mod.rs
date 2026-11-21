//! 出典ごとの行番号を保って天体を検索する。ヘッダ・欠測の有無はカタログごとに異なる。
mod app;
mod catalog;
mod depth;
pub(crate) mod display;
pub(crate) mod horizon;
mod layers;
mod satellite;
pub(crate) mod selections;
mod spacecraft;
use crate::catalog::SkyData;
use catalog::{error, Catalog};
use eframe::egui;
use std::{
    io,
    path::{Path, PathBuf},
    rc::Rc,
};

#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Default)]
enum Kind {
    #[default]
    Stars,
    Constellations,
    Ngc,
    Galaxies,
    Quasars,
    Satellites,
    Small,
    Spacecraft,
}
impl Kind {
    const ALL: [Self; 8] = [
        Self::Stars,
        Self::Constellations,
        Self::Ngc,
        Self::Galaxies,
        Self::Quasars,
        Self::Satellites,
        Self::Small,
        Self::Spacecraft,
    ];
    fn label(self) -> &'static str {
        match self {
            Self::Stars => "恒星の固有名",
            Self::Constellations => "星座",
            Self::Ngc => "NGC / IC",
            Self::Galaxies => "SDSS 銀河",
            Self::Quasars => "SDSS クエーサー",
            Self::Satellites => "自然衛星",
            Self::Small => "小惑星・太陽系外縁天体・彗星",
            Self::Spacecraft => "探査機",
        }
    }
}
#[derive(Clone, Copy, Debug)]
enum Position {
    Sky([f64; 2], Option<i32>),
    Solar([f64; 3], f64),
}
impl Position {
    fn equatorial(coords: [f64; 2], hip: Option<i32>) -> io::Result<Self> {
        if coords[0].is_finite() && (-90. ..=90.).contains(&coords[1]) {
            Ok(Self::Sky([coords[0].rem_euclid(360.), coords[1]], hip))
        } else {
            Err(error("カタログの赤経・赤緯を確定できません"))
        }
    }
    fn solar(p: [f64; 3], distance: f64) -> io::Result<Self> {
        if p.iter().all(|v| v.is_finite()) {
            Ok(Self::Solar(p, distance))
        } else {
            Err(error("太陽中心座標が不正です"))
        }
    }
}
#[derive(Default)]
pub(crate) struct Browser {
    horizon: horizon::Layer,
    pub open: bool,
    pub(crate) selections: selections::Selections,
    kind: Kind,
    root: Option<PathBuf>,
    data: Option<Rc<Catalog>>,
    loading: bool,
    catalogs: display::Catalogs,
    query: String,
    filtered: Option<Vec<usize>>,
    active: Option<usize>,
    epoch: Option<[f64; 3]>,
    marker: Option<(String, Position)>,
    error: Option<String>,
    follow: bool,
    focus_next: bool,
    pub(crate) display: display::Display,
}
impl Browser {
    pub(crate) fn stop_tracking(&mut self) {
        self.follow = false;
    }

    fn load(&mut self, root: PathBuf, ctx: &egui::Context) {
        self.root = Some(root.clone());
        self.data = None;
        self.active = None;
        self.marker = None;
        self.epoch = None;
        self.filtered = None;
        self.error = None;
        self.loading = true;
        self.poll(ctx);
    }
    fn scene_catalog(
        &mut self,
        kind: Kind,
        root: &Path,
        ctx: &egui::Context,
    ) -> Option<Rc<Catalog>> {
        match self.catalogs.get(kind, root, ctx) {
            Ok(c) => c,
            Err(e) => {
                self.error = Some(e);
                None
            }
        }
    }
    fn select_scene(&mut self, kind: Kind, index: usize, c: Rc<Catalog>, open: bool) {
        if self.kind != kind {
            self.query.clear();
            self.filtered = None;
        }
        self.kind = kind;
        self.loading = false;
        self.data = Some(c);
        self.active = Some(index);
        self.epoch = None;
        self.marker = None;
        self.open |= open;
    }
    pub(crate) fn restore_display(v: &serde_json::Value) -> display::Display {
        display::Display::restore(v)
    }
    pub(crate) fn inspect_sirius(&mut self, sky: &SkyData) -> io::Result<()> {
        self.inspect(sky, Kind::Stars, "シリウス")
    }
    pub(crate) fn inspect_voyager(&mut self, sky: &SkyData) -> io::Result<()> {
        self.display.craft = true;
        self.display.groups[4].paths = true;
        self.display.groups[4].labels = true;
        self.inspect(sky, Kind::Spacecraft, "Voyager I")
    }
    fn inspect(&mut self, sky: &SkyData, kind: Kind, name: &str) -> io::Result<()> {
        self.kind = kind;

        self.root = Some(sky.root.clone());
        let c = Catalog::load(self.kind, &sky.root)?;
        self.query = name.to_owned();
        self.filtered = c.search(&self.query);
        self.active = Some(
            *self
                .filtered
                .as_ref()
                .and_then(|rows| rows.first())
                .ok_or_else(|| error("検証対象の天体を検索できません"))?,
        );
        self.epoch = None;
        self.follow = true;
        self.open = true;
        let c = Rc::new(c);
        self.catalogs.insert(&sky.root, kind, c.clone());
        self.data = Some(c);
        Ok(())
    }
    fn poll(&mut self, ctx: &egui::Context) {
        if !self.loading {
            return;
        }
        let Some(root) = self.root.as_deref() else {
            return;
        };
        match self.catalogs.get(self.kind, root, ctx) {
            Ok(Some(c)) => {
                self.filtered = c.search(&self.query);
                self.data = Some(c);
                self.loading = false;
            }
            Ok(None) => {}
            Err(e) => {
                self.error = Some(e);
                self.loading = false;
            }
        }
    }
    fn panel(&mut self, ctx: &egui::Context, sky: &SkyData, epoch: [f64; 3]) -> Option<Position> {
        if self.open && self.root.as_ref() != Some(&sky.root) {
            self.load(sky.root.clone(), ctx);
        }
        self.poll(ctx);
        let mut open = self.open;
        let mut apply = std::mem::take(&mut self.focus_next);
        egui::Window::new("天体ブラウザ")
            .open(&mut open)
            .default_width(420.)
            .show(ctx, |ui| {
                let before = self.kind;
                ui.add_enabled_ui(!self.loading, |ui| {
                    egui::ComboBox::from_label("カタログ")
                        .selected_text(self.kind.label())
                        .show_ui(ui, |ui| {
                            for k in Kind::ALL {
                                ui.selectable_value(&mut self.kind, k, k.label());
                            }
                        });
                });
                if self.kind != before {
                    self.query.clear();
                    self.load(sky.root.clone(), ctx);
                }
                if self.loading {
                    ui.spinner();
                    ui.label("カタログを読み込んでいます");
                    return;
                }
                if let Some(c) = &self.data {
                    if ui
                        .add(egui::TextEdit::singleline(&mut self.query).hint_text(
                            if matches!(self.kind, Kind::Galaxies | Kind::Quasars) {
                                "原ファイルの記録番号 (1 始まり)"
                            } else {
                                "名称・番号・原ファイルを検索"
                            },
                        ))
                        .changed()
                    {
                        self.filtered = c.search(&self.query);
                    }
                    let count = self.filtered.as_ref().map_or_else(|| c.len(), Vec::len);
                    ui.label(format!("{count} / {} 件", c.len()));
                    let height = ui.text_style_height(&egui::TextStyle::Body) + 4.;
                    egui::ScrollArea::vertical()
                        .id_salt("object-results")
                        .max_height(210.)
                        .show_rows(ui, height, count, |ui, range| {
                            for index in range {
                                let i = self.filtered.as_ref().map_or(index, |rows| rows[index]);
                                let row = ui.selectable_label(Some(i) == self.active, c.title(i));
                                let mut center = row.clicked();
                                row.context_menu(|ui| {
                                    if ui.button("中央に表示").clicked() {
                                        center = true;
                                        ui.close_menu();
                                    }
                                });
                                if center {
                                    self.active = Some(i);
                                    self.marker = None;
                                    self.error = None;
                                    self.epoch = None;
                                    apply = true;
                                }
                            }
                        });
                    if matches!(self.kind, Kind::Spacecraft | Kind::Satellites | Kind::Small) {
                        self.display.ui(ui);
                    }
                    if let Some(i) = self.active {
                        ui.separator();
                        ui.label(c.info(i, epoch[0]));
                        if let Some((src, ord)) = c.identity(i) {
                            let mut selected = self.selections.contains(self.kind, src, ord);
                            if ui
                                .checkbox(&mut selected, "表示する天体の集合に追加")
                                .changed()
                            {
                                self.selections.toggle(self.kind, src, ord, selected);
                            }
                        }
                        ui.horizontal(|ui| {
                            apply |= ui.button("視点を移動").clicked();
                            if ui
                                .checkbox(&mut self.follow, "日時・観測地点に追従")
                                .changed()
                                && self.follow
                            {
                                apply = true;
                            }
                        });
                    }
                    if !c.warnings.is_empty() {
                        ui.collapsing(
                            format!("読込時の診断 {} 件", c.warnings.len()),
                            |ui| {
                                egui::ScrollArea::vertical()
                                    .id_salt("object-errors")
                                    .max_height(110.)
                                    .show(ui, |ui| {
                                        for s in &c.warnings {
                                            ui.label(s);
                                        }
                                    });
                            },
                        );
                    }
                }
                if let Some(err) = &self.error {
                    ui.colored_label(egui::Color32::LIGHT_RED, err);
                }
            });
        self.open = open;
        let i = self.active?;
        if !(apply || self.follow && self.epoch != Some(epoch)) {
            return None;
        }
        self.epoch = Some(epoch);
        let c = self.data.as_ref()?;
        match c.locate(i, sky, epoch[0]) {
            Ok(p) => {
                self.error = None;
                self.marker = Some((c.title(i), p));
                Some(p)
            }
            Err(e) => {
                self.marker = None;
                self.error = Some(e.to_string());
                None
            }
        }
    }
}

pub(crate) struct ScriptTarget {
    pub(crate) surface_parallax: bool,
    catalog: Catalog,
    row: usize,
}
impl ScriptTarget {
    pub(crate) fn open(name: &str, sky: &SkyData, browser: &Browser) -> io::Result<Self> {
        let (prefix, key) = name
            .split_once('.')
            .ok_or_else(|| error("対象IDが不正です"))?;
        let kind = match prefix {
            "Satellite" => Kind::Satellites,
            "Spacecraft" => Kind::Spacecraft,
            "Asteroids" | "Comet" | "TNO" => Kind::Small,
            _ => return Err(error("対象カタログを利用できません")),
        };
        let catalog = Catalog::load(kind, &sky.root)?;
        let preferred = browser
            .active
            .and_then(|i| browser.data.as_ref()?.satellite_source(i));
        let row = catalog.script_row(prefix, key, preferred)?;
        Ok(Self {
            catalog,
            row,
            surface_parallax: matches!(kind, Kind::Small | Kind::Spacecraft),
        })
    }
    pub(crate) fn position(&self, sky: &SkyData, jd: f64) -> io::Result<[f64; 3]> {
        match self.catalog.locate(self.row, sky, jd)? {
            Position::Solar(p, _) => Ok(p),
            _ => Err(error("日心座標がありません")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "MITAKA_DATA_ROOT に比較元データを指定して実行"]
    fn supplied_catalogs_find_and_track_objects() {
        let root = PathBuf::from(std::env::var_os("MITAKA_DATA_ROOT").expect("MITAKA_DATA_ROOT"));
        let sky = SkyData::open(&root).unwrap();
        let cases = [
            (Kind::Stars, "シリウス"),
            (Kind::Constellations, "Orion"),
            (Kind::Ngc, "NGC 1976"),
            (Kind::Galaxies, "1"),
            (Kind::Quasars, "1"),
            (Kind::Satellites, "Phobos"),
            (Kind::Satellites, "Charon"),
            (Kind::Satellites, "Nix"),
            (Kind::Small, "Ceres"),
            (Kind::Spacecraft, "Voyager I"),
        ];
        for (kind, query) in cases {
            let c = Catalog::load(kind, &root).unwrap();
            let matches = c.search(query).unwrap();
            eprintln!(
                "{kind:?} · {} records · {} hits · {} warnings",
                c.len(),
                matches.len(),
                c.warnings.len()
            );
            assert!(!matches.is_empty(), "{kind:?} {query}");
            let i = matches
                .into_iter()
                .find(|i| c.locate(*i, &sky, crate::J2000).is_ok())
                .unwrap_or_else(|| {
                    panic!(
                        "{kind:?} {query} · {}",
                        c.locate(c.search(query).unwrap()[0], &sky, crate::J2000)
                            .unwrap_err()
                    )
                });
            match kind {
                Kind::Satellites if query == "Charon" => {
                    assert_eq!(
                        (0..c.len())
                            .filter(|&index| c.locate(index, &sky, crate::J2000).is_ok())
                            .count(),
                        330
                    );
                }
                Kind::Spacecraft => {
                    assert_eq!(
                        (0..c.len())
                            .filter_map(|index| c.craft_path(index))
                            .map(<[_]>::len)
                            .sum::<usize>(),
                        110_810
                    );
                }
                Kind::Small => {
                    let path = c.small_path(i, display::Sampling::default()).unwrap();
                    assert_eq!(path.len(), 201);
                    let points: Vec<_> = path.iter().copied().collect::<Option<_>>().unwrap();
                    assert!(points[0]
                        .iter()
                        .zip(*points.last().unwrap())
                        .all(|(a, b)| (a - b).abs() < 1e-9));
                }
                _ => {}
            }
            match c.locate(i, &sky, crate::J2000).unwrap() {
                Position::Sky(p, _) => assert!(p[0].is_finite() && p[1].is_finite()),
                Position::Solar(p, _) => {
                    let Position::Solar(next, _) = c.locate(i, &sky, crate::J2000 + 1.).unwrap()
                    else {
                        panic!()
                    };
                    assert_ne!(p, next, "{kind:?} must update on date change");
                }
            }
        }
    }
}
