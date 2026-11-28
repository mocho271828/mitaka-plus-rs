pub(crate) mod image;
mod registry;
mod script;
mod worker;
use eframe::egui;
use registry::{Entry, Grant, Kind, Registry};
use std::{
    io,
    path::{Path, PathBuf},
    sync::mpsc,
    time::{Duration, Instant},
};
#[derive(Default)]
pub(super) struct Media {
    open: bool,
    pub browser: bool,
    sound: bool,
    registrations: Registry,
    highlight: Option<usize>,
    selected: [Option<usize>; 3],
    active: Option<usize>,
    status: String,
    image: Option<image::Preview>,
    loading: Option<mpsc::Receiver<io::Result<egui::ColorImage>>>,
    player: Option<worker::Worker>,
    polled: Option<Instant>,
}
impl Media {
    pub fn reset(&mut self, root: &Path) {
        *self = Self {
            registrations: Registry::load(root),
            ..Self::default()
        };
    }
    fn pick() -> Option<PathBuf> {
        rfd::FileDialog::new()
            .set_title("表示するローカルファイルを選択")
            .add_filter(
                "画像・音声・動画",
                &[
                    "png", "jpg", "jpeg", "mp4", "m4v", "mov", "webm", "avi", "wmv", "mpg", "mpeg",
                    "ogv", "mp3", "wav", "m4a", "aac", "flac", "aif", "aiff", "caf", "ogg", "oga",
                    "wma",
                ],
            )
            .pick_file()
    }
    pub fn choose(&mut self, ctx: &egui::Context) {
        let Some(path) = Self::pick() else { return };
        self.open = true;
        let result = Grant::open(&path).and_then(|g| {
            let index = self.registrations.entries.len();
            self.registrations.entries.push(Entry::direct(g));
            self.activate(index, ctx)
        });
        if let Err(e) = result {
            self.status = e.to_string();
        }
    }
    fn slot(kind: Kind) -> io::Result<usize> {
        match kind {
            Kind::Image => Ok(0),
            Kind::Movie => Ok(1),
            Kind::Audio => Ok(2),
            Kind::Unknown => Err(io::Error::other("未対応の媒体種類です")),
        }
    }
    fn active_kind(&self) -> Option<Kind> {
        self.active
            .map(|idx| self.registrations.entries[idx].effective())
    }
    fn close(&mut self) {
        self.player = None;
        self.loading = None;
        self.image = None;
        self.active = None;
        self.open = false;
        self.polled = None;
    }
    fn activate(&mut self, idx: usize, ctx: &egui::Context) -> io::Result<()> {
        let entry = &self.registrations.entries[idx];
        let grant = entry
            .grant
            .as_ref()
            .ok_or_else(|| io::Error::other("登録一覧でこのファイルを選択してください"))?;
        let kind = entry.effective();
        let slot = Self::slot(kind)?;
        let path = grant.path.clone();
        let file = grant.file.try_clone()?;
        self.close();
        self.status.clear();
        self.open = true;
        self.active = Some(idx);
        self.selected[slot] = Some(idx);
        self.highlight = Some(idx);
        if kind == Kind::Image {
            let (tx, rx) = mpsc::channel();
            self.loading = Some(rx);
            let context = ctx.clone();
            std::thread::spawn(move || {
                let _ = tx.send(image::load_selected(&path, file));
                context.request_repaint();
            });
        } else {
            match worker::Worker::start(file, &path) {
                Ok(w) => self.player = Some(w),
                Err(e) => {
                    self.close();
                    return Err(e);
                }
            }
        }
        Ok(())
    }
    fn approve(&mut self, idx: usize, map: bool, ctx: &egui::Context) -> io::Result<()> {
        if map || self.registrations.entries[idx].grant.is_none() {
            let g = if map {
                let Some(p) = Self::pick() else { return Ok(()) };
                Grant::open(&p)?
            } else {
                self.registrations.candidate_grant(idx)?
            };
            let row = &mut self.registrations.entries[idx];
            if row.kind != Kind::Unknown && row.kind != Kind::of(&g.path) {
                return Err(io::Error::other("登録種類と選択した媒体が一致しません"));
            }
            row.grant = Some(g);
        }
        self.activate(idx, ctx)
    }
    fn registrations_ui(&mut self, ctx: &egui::Context) {
        let mut browser = self.browser;
        let mut action = None;
        let mut direct = false;
        egui::Window::new("ローカル媒体の登録")
            .open(&mut browser)
            .default_width(560.)
            .show(ctx, |ui| {
                for msg in &self.registrations.issues {
                    ui.label(msg);
                }
                egui::ScrollArea::vertical()
                    .max_height(250.)
                    .show(ui, |ui| {
                        for (i, row) in self.registrations.entries.iter().enumerate() {
                            let text = format!(
                                "{}　{}　{}",
                                row.effective().label(),
                                row.name,
                                if row.grant.is_some() {
                                    "選択済み"
                                } else if row.resolved.is_some() {
                                    "参照あり"
                                } else {
                                    "未解決"
                                }
                            );
                            let r = ui.selectable_label(self.highlight == Some(i), text);
                            if r.clicked() {
                                self.highlight = Some(i);
                            }
                            if r.double_clicked() {
                                action = Some((i, false));
                            }
                        }
                    });
                if let Some(i) = self.highlight {
                    let e = &self.registrations.entries[i];
                    ui.label(format!(
                        "{}　{}　{} / {}",
                        e.source, e.title, e.descriptor, e.secondary
                    ));
                    if !e.paths.is_empty() {
                        ui.label(e.paths.join("\n"));
                    }
                    if !e.issue.is_empty() {
                        ui.label(&e.issue);
                    }
                    ui.horizontal(|ui| {
                        if ui.button("このファイルを表示").clicked() {
                            action = Some((i, false));
                        }
                        if ui.button("参照ファイルを指定…").clicked() {
                            action = Some((i, true));
                        }
                    });
                }
                if ui.button("ローカル媒体を追加…").clicked() {
                    direct = true;
                }
            });
        self.browser = browser;
        if let Some((i, map)) = action {
            if let Err(e) = self.approve(i, map, ctx) {
                self.open = true;
                self.status = e.to_string();
            }
        }
        if direct {
            self.choose(ctx);
        }
    }
    fn playback_ui(&mut self, ui: &mut egui::Ui) {
        let Some(w) = self.player.as_ref() else {
            return;
        };
        let mut command = None;
        ui.horizontal(|ui| {
            if ui.button("再生").clicked() {
                command = Some(serde_json::json!({"op":"play"}));
            }
            if ui.button("一時停止").clicked() {
                command = Some(serde_json::json!({"op":"pause"}));
            }
        });
        if let (Some(mut frame), Some(n)) =
            (w.state["frame"].as_u64(), w.state["frameLength"].as_u64())
        {
            if ui
                .add(egui::Slider::new(&mut frame, 0..=n).text("フレーム"))
                .changed()
            {
                command = Some(serde_json::json!({"op":"frame","frame":frame}));
            }
        } else if let (Some(mut seconds), Some(duration)) =
            (w.state["seconds"].as_f64(), w.state["duration"].as_f64())
        {
            if ui
                .add(egui::Slider::new(&mut seconds, 0. ..=duration).text("秒"))
                .changed()
            {
                command = Some(serde_json::json!({"op":"seek","seconds":seconds}));
            }
        }
        if let Some(c) = command {
            if let Err(e) = w.request(c) {
                self.status = e.to_string();
            }
        }
    }
    fn preview_contents(&mut self, ui: &mut egui::Ui) {
        if let Some(i) = self.active {
            let e = &self.registrations.entries[i];
            ui.label(&e.name);
            if let Some(g) = &e.grant {
                ui.label(g.path.display().to_string());
            }
        }
        if let Some(img) = &mut self.image {
            img.show(ui);
        }
        if self.loading.is_some() {
            ui.spinner();
        }
        self.playback_ui(ui);
        if !self.status.is_empty() {
            ui.label(&self.status);
        }
    }
    fn preview_window(&mut self, ctx: &egui::Context) {
        let mut open = true;
        if self.active_kind() == Some(Kind::Image) {
            ctx.show_viewport_immediate(
                egui::ViewportId::from_hash_of("mitaka-image-preview"),
                egui::ViewportBuilder::default()
                    .with_title("Mitaka Plus — 画像プレビュー")
                    .with_inner_size([600., 540.])
                    .with_min_inner_size([320., 280.]),
                |child, class| {
                    if class == egui::ViewportClass::Embedded {
                        egui::Window::new("画像プレビュー")
                            .open(&mut open)
                            .show(child, |ui| self.preview_contents(ui));
                    } else if child.input(|i| i.viewport().close_requested()) {
                        open = false;
                    } else {
                        egui::CentralPanel::default().show(child, |ui| self.preview_contents(ui));
                    }
                },
            );
        } else {
            egui::Window::new("メディアのプレビュー")
                .open(&mut open)
                .default_width(520.)
                .show(ctx, |ui| self.preview_contents(ui));
        }
        if !open {
            self.close();
        }
    }
    pub fn frame(&mut self, ctx: &egui::Context) {
        if let Some(rx) = &self.loading {
            match rx.try_recv() {
                Ok(Ok(img)) => {
                    self.loading = None;
                    self.image = Some(image::Preview::new(ctx, img));
                }
                Ok(Err(e)) => {
                    self.loading = None;
                    self.active = None;
                    self.status = e.to_string();
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.loading = None;
                    self.active = None;
                    self.status = "画像の読み込みが中断しました".into();
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        if let Some(w) = self.player.as_mut() {
            if self
                .polled
                .is_none_or(|t| t.elapsed() >= Duration::from_millis(180))
            {
                self.polled = Some(Instant::now());
                match w.refresh() {
                    Ok(true) => {}
                    r => {
                        self.status = r
                            .err()
                            .map_or_else(|| "プレーヤーを閉じました".into(), |e| e.to_string());
                        self.player = None;
                        self.active = None;
                    }
                }
            }
            ctx.request_repaint_after(Duration::from_millis(180));
        }
        self.registrations_ui(ctx);
        if self.open {
            self.preview_window(ctx);
        }
        let mut sound = self.sound;
        egui::Window::new("サウンドコントローラー")
            .open(&mut sound)
            .show(ctx, |ui| {
                if self.active_kind() == Some(Kind::Audio) {
                    self.playback_ui(ui);
                } else {
                    ui.label("登録一覧から音声を選択してください。");
                }
                if ui.button("登録を開く").clicked() {
                    self.browser = true;
                }
            });
        self.sound = sound;
    }
}
