//! カタログ座標と距離をpcで扱う。天の川の簡略表示は原版のpreviewと同じ原座標。
mod radiative;
use eframe::egui::{self, Align2, Color32, FontId, Pos2};
use mitaka_core::galaxy_radiative::RadiativeModel;
use mitaka_core::{
    extended_catalog::{load_sdss, SdssKind},
    galaxy::{self, OurGalaxyModel},
};
use std::sync::Arc;
use std::{
    io,
    path::{Path, PathBuf},
    sync::mpsc,
};

#[derive(Clone, Copy, Debug, PartialEq)]
enum Kind {
    MilkyWay,
    Clusters,
    Nearby,
    Sdss,
    Quasars,
}
impl Kind {
    const ALL: [Self; 5] = [
        Self::MilkyWay,
        Self::Clusters,
        Self::Nearby,
        Self::Sdss,
        Self::Quasars,
    ];
    fn name(self) -> &'static str {
        match self {
            Self::MilkyWay => "天の川銀河",
            Self::Clusters => "球状星団",
            Self::Nearby => "近傍銀河",
            Self::Sdss => "SDSS 銀河",
            Self::Quasars => "SDSS クエーサー",
        }
    }
    fn file(self) -> &'static str {
        match self {
            Self::MilkyWay => "ourgalaxy_model.dat",
            Self::Clusters => "globular_cluster.dat",
            Self::Nearby => "NBG.dat",
            Self::Sdss => "SDSS_GAL.dat",
            Self::Quasars => "SDSS_QSO.dat",
        }
    }
}
struct Object {
    position: [f32; 3],
    row: usize,
    group: u8,
    name: Option<String>,
}
impl Object {
    fn label(&self, kind: Kind) -> String {
        self.name.clone().unwrap_or_else(|| {
            if kind == Kind::MilkyWay {
                format!("点群 {} · 原行 {}", self.group + 1, self.row)
            } else {
                format!("{} · 原行 {}", kind.name(), self.row)
            }
        })
    }
}
struct Objects {
    radiation: Option<io::Result<Arc<RadiativeModel>>>,
    rows: Vec<Object>,
    source_rows: usize,
    radius: f64,
}
fn objects(kind: Kind, root: &Path) -> io::Result<Objects> {
    let path = root.join(kind.file());
    let mut rows = Vec::new();
    let mut source_rows = 0;
    let mut radiation = None;
    let mut add = |row: usize, ra: f32, dec: f32, distance: f32, name: Option<String>| {
        source_rows += 1;
        if let Some(p) = galaxy::evaluate_position_pc(ra, dec, distance).position {
            rows.push(Object {
                position: p.ecliptic.map(|x| x as f32),
                row,
                group: 0,
                name,
            });
        }
    };
    match kind {
        Kind::Clusters => {
            for (i, g) in galaxy::load_globular_clusters(&path)?
                .records
                .into_iter()
                .enumerate()
            {
                let name = String::from_utf8_lossy(&g.name_bytes)
                    .trim_matches(['\0', ' '])
                    .to_owned();
                add(i + 1, g.ra_deg, g.dec_deg, g.distance_pc, Some(name));
            }
        }
        Kind::Nearby => {
            for (i, g) in galaxy::load_nearby_galaxies(&path)?
                .records
                .into_iter()
                .enumerate()
            {
                let code = if g.catalog_code.is_ascii_graphic() {
                    char::from(g.catalog_code).to_string()
                } else {
                    g.catalog_code.to_string()
                };
                add(
                    i + 1,
                    g.ra_deg,
                    g.dec_deg,
                    g.distance_pc,
                    Some(format!("{code}{} · 原行 {}", g.catalog_number, i + 1)),
                );
            }
        }
        Kind::Sdss | Kind::Quasars => {
            let family = if kind == Kind::Sdss {
                SdssKind::Galaxy
            } else {
                SdssKind::Quasar
            };
            for (i, g) in load_sdss(&path, family)?.into_iter().enumerate() {
                add(i + 1, g.ra_deg, g.dec_deg, g.distance_pc, None);
            }
        }
        Kind::MilkyWay => {
            let model = OurGalaxyModel::load(&path)?;
            radiation = Some(RadiativeModel::from_model(&model).map(Arc::new));
            for (group, cloud) in model.clouds.into_iter().enumerate() {
                for (i, position) in cloud.positions_pc.into_iter().enumerate() {
                    source_rows += 1;
                    if position.iter().all(|p| p.is_finite()) {
                        rows.push(Object {
                            position,
                            row: i + 1,
                            group: group as u8,
                            name: None,
                        });
                    }
                }
            }
        }
    }
    let radius = rows
        .iter()
        .map(|o| o.position.map(f64::from))
        .map(|p| p[0].hypot(p[1]).hypot(p[2]))
        .fold(1_f64, f64::max);
    Ok(Objects {
        radiation,
        rows,
        source_rows,
        radius,
    })
}

pub struct Browser {
    show_radiation: bool,
    radiation: radiative::Radiation,
    root: PathBuf,
    kind: Kind,
    objects: Option<Objects>,
    pending: Option<mpsc::Receiver<io::Result<Objects>>>,
    error: Option<String>,
    pub controls: bool,
    yaw: f64,
    pitch: f64,
    zoom: f64,
    center: [f64; 3],
    selected: Option<usize>,
    query: String,
    note: String,
}
impl Browser {
    pub fn new(root: PathBuf, ctx: &egui::Context) -> Self {
        let mut result = Self {
            show_radiation: false,
            radiation: Default::default(),
            root,
            kind: Kind::MilkyWay,
            objects: None,
            pending: None,
            error: None,
            controls: false,
            yaw: 0.55,
            pitch: 0.65,
            zoom: 1.,
            center: [0.; 3],
            selected: None,
            query: String::new(),
            note: String::new(),
        };
        result.load(ctx);
        result
    }
    fn load(&mut self, ctx: &egui::Context) {
        let kind = self.kind;
        let root = self.root.clone();
        let ctx = ctx.clone();
        let (tx, rx) = mpsc::channel();
        self.pending = Some(rx);
        self.objects = None;
        self.radiation = Default::default();
        self.error = None;
        self.selected = None;
        self.center = [0.; 3];
        self.zoom = 1.;
        self.note.clear();
        std::thread::spawn(move || {
            let _ = tx.send(objects(kind, &root));
            ctx.request_repaint();
        });
    }
    fn collect(&mut self) {
        if let Some(rx) = &self.pending {
            match rx.try_recv() {
                Ok(result) => {
                    self.pending = None;
                    match result {
                        Ok(o) => self.objects = Some(o),
                        Err(e) => self.error = Some(e.to_string()),
                    }
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.pending = None;
                    self.error = Some("読込処理が終了しました".into());
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
    }
    fn search(&mut self) {
        self.note.clear();
        let query = self.query.trim().to_lowercase();
        if query.is_empty() {
            return;
        }
        if let Some(o) = &self.objects {
            let row = query.parse::<usize>().ok();
            let found = o.rows.iter().position(|r| {
                row == Some(r.row)
                    || r.name
                        .as_ref()
                        .is_some_and(|name| name.to_lowercase().contains(&query))
            });
            if let Some(i) = found {
                self.selected = Some(i);
                self.center = o.rows[i].position.map(f64::from);
            } else {
                self.note = "該当するレコードがありません".into();
            }
        }
    }
    pub fn ready(&self) -> io::Result<bool> {
        if let Some(error) = &self.error {
            return Err(io::Error::other(error.clone()));
        }
        Ok(self.objects.is_some())
    }
    pub fn panel(&mut self, ctx: &egui::Context) {
        self.collect();
        let mut controls = self.controls;
        egui::Window::new("銀河カタログ")
            .open(&mut controls)
            .show(ctx, |ui| {
                let previous = self.kind;
                egui::ComboBox::from_label("表示するカタログ")
                    .selected_text(self.kind.name())
                    .show_ui(ui, |ui| {
                        for kind in Kind::ALL {
                            ui.selectable_value(&mut self.kind, kind, kind.name());
                        }
                    });
                if previous != self.kind {
                    self.load(ctx);
                }
                if self.kind == Kind::MilkyWay {
                    ui.checkbox(&mut self.show_radiation, "銀河の放射・吸収");
                    if self.show_radiation {
                        ui.label(format!(
                            "銀河中心からの視点距離 {:.0} pc",
                            (40000. / self.zoom).clamp(1., 1e7)
                        ));
                        if ui.button("全体を表示").clicked() {
                            self.zoom = 1.;
                        }
                        return;
                    }
                }
                ui.horizontal(|ui| {
                    ui.label("原行番号・名称");
                    let input = ui.text_edit_singleline(&mut self.query);
                    if (input.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)))
                        || ui.button("検索").clicked()
                    {
                        self.search();
                    }
                });
                if let Some(o) = &self.objects {
                    ui.label(format!(
                        "{} · 有効位置 {} / 原行 {} 件",
                        self.kind.file(),
                        o.rows.len(),
                        o.source_rows
                    ));
                    if let Some(p) = self.selected.and_then(|i| o.rows.get(i)) {
                        ui.label(format!(
                            "{}\nX {:.2}  Y {:.2}  Z {:.2} pc",
                            p.label(self.kind),
                            p.position[0],
                            p.position[1],
                            p.position[2]
                        ));
                    }
                }
                if !self.note.is_empty() {
                    ui.label(&self.note);
                }
                if ui.button("全体を表示").clicked() {
                    self.center = [0.; 3];
                    self.zoom = 1.;
                }
            });
        self.controls = controls;
    }
    pub fn draw(&mut self, ui: &mut egui::Ui, picking: crate::viewer::Picking) {
        self.collect();
        let Some(objects) = &self.objects else {
            ui.centered_and_justified(|ui| {
                ui.label(self.error.as_deref().unwrap_or("銀河カタログを読み込み中…"))
            });
            return;
        };
        let (res, painter) =
            ui.allocate_painter(ui.available_size(), egui::Sense::click_and_drag());
        let rect = res.rect;
        if res.dragged() {
            let d = res.drag_delta();
            self.yaw -= 0.006 * f64::from(d.x);
            self.pitch = (self.pitch + 0.006 * f64::from(d.y)).clamp(-1.56, 1.56);
        }
        if res.hovered() {
            self.zoom = (self.zoom
                * ui.input(|i| {
                    f64::from(
                        (i.smooth_scroll_delta.y * 0.008).clamp(-2., 2.).exp() * i.zoom_delta(),
                    )
                }))
            .clamp(0.05, 1e6);
        }
        if self.kind == Kind::MilkyWay && self.show_radiation {
            match &objects.radiation {
                Some(Ok(model)) => self
                    .radiation
                    .draw(&painter, model, rect, self.yaw, self.pitch, self.zoom),
                Some(Err(e)) => {
                    painter.text(
                        rect.center(),
                        Align2::CENTER_CENTER,
                        e.to_string(),
                        FontId::proportional(14.),
                        Color32::LIGHT_GRAY,
                    );
                }
                None => {}
            }
            return;
        }
        let size = f64::from(rect.width().min(rect.height())) * 0.46 * self.zoom / objects.radius;
        let (sy, cy) = self.yaw.sin_cos();
        let (sp, cp) = self.pitch.sin_cos();
        let project = |p: [f32; 3]| {
            let x = f64::from(p[0]) - self.center[0];
            let y = f64::from(p[1]) - self.center[1];
            let z = f64::from(p[2]) - self.center[2];
            rect.center()
                + egui::vec2(
                    ((cy * x + sy * y) * size) as f32,
                    (-((-sy * x + cy * y) * sp + cp * z) * size) as f32,
                )
        };
        let stride = objects.rows.len().div_ceil(32000).max(1);
        let pointer = picking.pointer(&res);
        let mut best = 64f32;
        for (i, p) in objects.rows.iter().enumerate().step_by(stride) {
            let xy = project(p.position);
            if rect.contains(xy) {
                let color = [
                    Color32::from_rgb(184, 197, 219),
                    Color32::from_rgb(139, 164, 203),
                    Color32::from_rgb(225, 180, 145),
                    Color32::from_rgb(186, 166, 134),
                ][usize::from(p.group)];
                painter.rect_filled(
                    egui::Rect::from_center_size(xy, egui::vec2(1.6, 1.6)),
                    0.,
                    color,
                );
                if let Some(click) = pointer {
                    let d = xy.distance_sq(click);
                    if d <= best {
                        best = d;
                        self.selected = Some(i);
                    }
                }
            }
        }
        if let Some(p) = self.selected.and_then(|i| objects.rows.get(i)) {
            let xy = project(p.position);
            painter.circle_stroke(xy, 6., egui::Stroke::new(1_f32, Color32::GOLD));
            painter.text(
                xy + egui::vec2(8., -4.),
                Align2::LEFT_BOTTOM,
                p.label(self.kind),
                FontId::proportional(13.),
                Color32::GOLD,
            );
        }
        let label = if stride > 1 {
            format!("{} · 表示間隔 {} 行", self.kind.name(), stride)
        } else {
            self.kind.name().into()
        };
        painter.text(
            Pos2::new(rect.left() + 12., rect.bottom() - 12.),
            Align2::LEFT_BOTTOM,
            label,
            FontId::proportional(13.),
            Color32::LIGHT_GRAY,
        );
    }
}
