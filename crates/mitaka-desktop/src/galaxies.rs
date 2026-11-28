//! 太陽系と銀河の共通カメラ。読み込み・画面投影の完了世代を一致させて選択する。
mod catalog;
mod radiative;
mod spatial;
use crate::{
    deep_sky::Display,
    solar::{precise::AU_PER_PC, SolarCamera},
    viewer::Picking,
};
use catalog::{objects, Kind, Objects};
use eframe::egui::{self, Color32, Painter, Pos2, Rect, TextureHandle};
pub(crate) use radiative::center;
use spatial::{Id, Projection, Raster, View};
use std::{
    io,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
};

#[derive(Default)]
enum Source {
    #[default]
    Unloaded,
    Loading(mpsc::Receiver<io::Result<Objects>>),
    Loaded(Arc<Objects>),
    Failed(String),
}
struct Job {
    view: View,
    receiver: mpsc::Receiver<Option<Raster>>,
    cancelled: Arc<AtomicBool>,
}
impl Drop for Job {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }
}
struct Frame {
    view: View,
    raster: Raster,
    texture: TextureHandle,
}
pub struct Browser {
    root: PathBuf,
    sources: [Source; 5],
    kind: Kind,
    pub controls: bool,
    radiation: radiative::Radiation,
    show_radiation: bool,
    local_visible: [bool; 3],
    selected: Option<Id>,
    query: String,
    note: String,
    frame: Option<Frame>,
    job: Option<Job>,
    expected: Option<View>,
}
impl Browser {
    pub fn new(root: PathBuf, _: &egui::Context) -> Self {
        Self {
            root,
            sources: Default::default(),
            kind: Kind::MilkyWay,
            controls: false,
            radiation: Default::default(),
            show_radiation: true,
            local_visible: [true; 3],
            selected: None,
            query: String::new(),
            note: String::new(),
            frame: None,
            job: None,
            expected: None,
        }
    }
    fn flag<'a>(&'a mut self, kind: Kind, display: &'a mut Display) -> &'a mut bool {
        match kind {
            Kind::Sdss => &mut display.galaxies,
            Kind::Quasars => &mut display.quasars,
            _ => &mut self.local_visible[kind as usize],
        }
    }
    fn catalog(&self, kind: Kind) -> Option<&Arc<Objects>> {
        if let Source::Loaded(cat) = &self.sources[kind as usize] {
            Some(cat)
        } else {
            None
        }
    }
    fn prepare(&mut self, ctx: &egui::Context, display: &Display) {
        for (kind, enabled) in Kind::ALL.into_iter().zip([
            self.local_visible[0],
            self.local_visible[1],
            self.local_visible[2],
            display.galaxies,
            display.quasars,
        ]) {
            let slot = &mut self.sources[kind as usize];
            if matches!(slot, Source::Unloaded) && (enabled || self.controls && kind == self.kind) {
                let root = self.root.clone();
                let ctx = ctx.clone();
                let (send, rx) = mpsc::channel();
                *slot = Source::Loading(rx);
                std::thread::spawn(move || {
                    let _ = send.send(objects(kind, &root));
                    ctx.request_repaint();
                });
            }
            if let Source::Loading(rx) = slot {
                match rx.try_recv() {
                    Ok(Ok(cat)) => *slot = Source::Loaded(Arc::new(cat)),
                    Ok(Err(e)) => *slot = Source::Failed(e.to_string()),
                    Err(mpsc::TryRecvError::Disconnected) => {
                        *slot = Source::Failed("読込処理が終了しました".into())
                    }
                    Err(mpsc::TryRecvError::Empty) => {}
                }
            }
        }
    }
    pub fn ready(&self) -> io::Result<bool> {
        if self.kind == Kind::MilkyWay && self.show_radiation {
            if let Some(view) = self.expected {
                if !self.radiation.ready(view.camera)? {
                    return Ok(false);
                }
            }
        }
        match &self.sources[self.kind as usize] {
            Source::Failed(e) => Err(io::Error::other(e.clone())),
            Source::Loaded(_) => Ok(self.frame.as_ref().is_some_and(|f| {
                Some(f.view) == self.expected && f.view.mask & (1 << self.kind as usize) != 0
            })),
            _ => Ok(false),
        }
    }
    pub(crate) fn select_sdss(&mut self, display: &mut Display) {
        self.kind = Kind::Sdss;
        display.galaxies = true;
    }
    pub fn open(&mut self, camera: &mut SolarCamera, rect: Rect) {
        self.controls = true;
        self.overview(camera, rect);
    }
    fn overview(&self, camera: &mut SolarCamera, rect: Rect) {
        let (center, radius) = if self.kind == Kind::MilkyWay {
            (radiative::center(), 8000.)
        } else {
            ([0.; 3], self.catalog(self.kind).map_or(8000., |o| o.radius))
        };
        let [w, h] = spatial::dimensions(rect);
        let t = (camera.fov * 0.5).to_radians().tan() * f64::from((w / h).min(1.));
        camera.set_target_pc(center);
        camera.distance = radius * 1_f64.hypot(t) / t * AU_PER_PC;
    }
    fn select_row(&mut self, camera: &mut SolarCamera) {
        let q = self.query.trim().to_lowercase();
        if q.is_empty() {
            return;
        }
        let row = q.parse::<usize>().ok();
        let found = self.catalog(self.kind).and_then(|c| {
            c.rows
                .iter()
                .enumerate()
                .find(|(_, o)| {
                    row == Some(o.row)
                        || o.name
                            .as_ref()
                            .is_some_and(|n| n.to_lowercase().contains(&q))
                })
                .map(|(i, o)| (i, o.position))
        });
        if let Some((i, p)) = found {
            self.selected = Some(Id(self.kind, i));
            camera.set_target_pc(p);
            self.note.clear();
        } else {
            self.note = "該当する位置付きレコードがありません".into();
        }
    }
    pub fn panel(
        &mut self,
        ctx: &egui::Context,
        camera: &mut SolarCamera,
        rect: Rect,
        display: &mut Display,
    ) {
        if !self.controls {
            return;
        }
        let mut open = true;
        egui::Window::new("銀河カタログ")
            .open(&mut open)
            .show(ctx, |ui| {
                for kind in Kind::ALL {
                    ui.horizontal(|ui| {
                        ui.checkbox(self.flag(kind, display), kind.name());
                        if ui.selectable_label(self.kind == kind, "検索対象").clicked() {
                            self.kind = kind;
                        }
                    });
                }
                ui.checkbox(&mut self.show_radiation, "天の川の放射・吸収");
                ui.horizontal(|ui| {
                    let input = ui.add(
                        egui::TextEdit::singleline(&mut self.query).hint_text("原行番号・名称"),
                    );
                    if (input.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)))
                        || ui.button("検索して中心へ").clicked()
                    {
                        *self.flag(self.kind, display) = true;
                        self.select_row(camera);
                    }
                });
                if let Some(c) = self.catalog(self.kind) {
                    ui.label(format!(
                        "{} · 有効 {} / 原行 {} 件",
                        self.kind.file(),
                        c.rows.len(),
                        c.source_rows
                    ));
                }
                if let Source::Failed(e) = &self.sources[self.kind as usize] {
                    ui.label(e);
                }
                if let Some(Id(k, i)) = self.selected {
                    if let Some(p) = self.catalog(k).and_then(|c| c.rows.get(i)) {
                        ui.label(format!(
                            "{}\nX {:.3}  Y {:.3}  Z {:.3} pc",
                            p.label(k),
                            p.position[0],
                            p.position[1],
                            p.position[2]
                        ));
                        if ui.button("選択した天体を中心へ").clicked() {
                            camera.set_target_pc(p.position);
                        }
                    }
                }
                if !self.note.is_empty() {
                    ui.label(&self.note);
                }
                if ui.button("全体を表示").clicked() {
                    self.overview(camera, rect);
                }
                if ui.button("太陽系を表示").clicked() {
                    *camera = SolarCamera::default();
                    self.selected = None;
                }
            });
        self.controls = open;
    }
    pub fn paint(
        &mut self,
        painter: &Painter,
        response: &egui::Response,
        camera: SolarCamera,
        display: &Display,
        picking: Picking,
        surface_view: bool,
    ) {
        let rect = response.rect;
        // GalaxyOverlay.mm は地表モードを抜けた時点で元の表示設定に戻す。
        let effective = Display {
            ngc: display.ngc,
            galaxies: display.galaxies && !surface_view,
            quasars: display.quasars && !surface_view,
        };
        self.prepare(painter.ctx(), &effective);
        let milky = self.local_visible[0] && radiative::galactic_view(camera).is_some();
        if milky && self.show_radiation {
            if let Some(Source::Loaded(cat)) = self.sources.first() {
                if let Some(Ok(m)) = &cat.radiation {
                    self.radiation.draw(painter, m, rect, camera);
                }
            }
        }
        let flags = [
            milky,
            self.local_visible[1],
            self.local_visible[2],
            effective.galaxies,
            effective.quasars,
        ];
        let catalogs: Vec<_> = [
            Kind::Sdss,
            Kind::Quasars,
            Kind::Clusters,
            Kind::Nearby,
            Kind::MilkyWay,
        ]
        .into_iter()
        .filter_map(|k| {
            if flags[k as usize] {
                self.catalog(k).map(|c| (k, Arc::clone(c)))
            } else {
                None
            }
        })
        .collect();
        let [width, height] = spatial::dimensions(rect);
        let mask = catalogs.iter().fold(0, |m, (k, _)| m | 1 << *k as usize);
        let view = View {
            camera,
            width,
            height,
            scale: painter.ctx().pixels_per_point(),
            mask,
        };
        self.expected = Some(view);
        if let Some(job) = &self.job {
            if job.view != view {
                job.cancelled.store(true, Ordering::Relaxed);
            }
            match job.receiver.try_recv() {
                Ok(result) => {
                    if let Some(mut raster) = result.filter(|_| job.view == view) {
                        let image = std::mem::replace(
                            &mut raster.image,
                            egui::ColorImage::new([0, 0], Color32::TRANSPARENT),
                        );
                        let texture = painter.ctx().load_texture(
                            "銀河カタログの投影",
                            image,
                            egui::TextureOptions::NEAREST,
                        );
                        self.frame = Some(Frame {
                            view,
                            raster,
                            texture,
                        });
                    }
                    self.job = None;
                }
                Err(mpsc::TryRecvError::Disconnected) => self.job = None,
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        if let Some(frame) = self.frame.as_ref().filter(|f| f.view == view) {
            painter.image(
                frame.texture.id(),
                rect,
                Rect::from_min_max(Pos2::ZERO, egui::pos2(1., 1.)),
                Color32::WHITE,
            );
            if let Some(click) = picking.pointer(response) {
                let q = click - rect.min;
                if let Some(hit) = frame.raster.pick(f64::from(q.x), f64::from(q.y), 6.) {
                    self.selected = Some(hit);
                }
            }
            if let Some(Id(k, i)) = self.selected.filter(|id| mask & (1 << id.0 as usize) != 0) {
                if let Some(p) = self.catalog(k).and_then(|c| c.rows.get(i)) {
                    if let Some(d) = Projection::new(view).shape(k, p) {
                        let xy = rect.min + egui::vec2(d.x as f32, d.y as f32);
                        painter.circle_stroke(xy, 6., egui::Stroke::new(1_f32, Color32::GOLD));
                        painter.text(
                            xy + egui::vec2(8., -4.),
                            egui::Align2::LEFT_BOTTOM,
                            p.label(k),
                            egui::FontId::proportional(13.),
                            Color32::GOLD,
                        );
                    }
                }
            }
        } else if self.job.is_none() && mask != 0 {
            let cancelled = Arc::new(AtomicBool::new(false));
            let flag = Arc::clone(&cancelled);
            let ctx = painter.ctx().clone();
            let (tx, receiver) = mpsc::channel();
            self.job = Some(Job {
                view,
                receiver,
                cancelled,
            });
            std::thread::spawn(move || {
                let _ = tx.send(Raster::render(view, &catalogs, &flag));
                ctx.request_repaint();
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entering_and_leaving_surface_masks_only_sdss_and_preserves_preferences() {
        let ctx = egui::Context::default();
        let mut browser = Browser::new(PathBuf::new(), &ctx);
        browser.local_visible = [false, false, true];
        let fixture = Arc::new(Objects {
            rows: vec![],
            source_rows: 0,
            radius: 1.,
            radiation: None,
        });
        for kind in [Kind::Nearby, Kind::Sdss, Kind::Quasars] {
            browser.sources[kind as usize] = Source::Loaded(fixture.clone());
        }
        for quasar_enabled in [true, false] {
            let prefs = Display {
                ngc: true,
                galaxies: true,
                quasars: quasar_enabled,
            };
            for surface in [false, true, false] {
                let _ = ctx.run(egui::RawInput::default(), |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        let (response, painter) =
                            ui.allocate_painter(egui::vec2(80., 60.), egui::Sense::hover());
                        browser.paint(
                            &painter,
                            &response,
                            Default::default(),
                            &prefs,
                            Picking::None,
                            surface,
                        );
                    });
                });
                let visible =
                    |kind: Kind| browser.expected.unwrap().mask & (1 << kind as usize) != 0;
                assert!(visible(Kind::Nearby));
                assert_eq!(visible(Kind::Sdss), !surface);
                assert_eq!(visible(Kind::Quasars), !surface && quasar_enabled);
                assert!(prefs.ngc && prefs.galaxies);
                assert_eq!(prefs.quasars, quasar_enabled);
            }
        }
    }
}
