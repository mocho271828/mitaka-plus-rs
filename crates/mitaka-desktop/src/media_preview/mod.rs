mod image;

use eframe::egui;
use std::{
    env, io,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::mpsc,
    time::Duration,
};

#[derive(Default)]
pub(super) struct Media {
    open: bool,
    path: PathBuf,
    status: String,
    image: Option<image::Preview>,
    loading: Option<mpsc::Receiver<io::Result<egui::ColorImage>>>,
    player: Option<Child>,
}

impl Media {
    fn stop_player(&mut self) {
        if let Some(mut child) = self.player.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
    pub fn choose(&mut self, ctx: &egui::Context) {
        self.open = true;
        if self.loading.is_some() {
            return;
        }
        if let Some(path) = rfd::FileDialog::new()
            .set_title("プレビューするローカルメディアを選択")
            .add_filter(
                "画像・音声・動画",
                &[
                    "png", "jpg", "jpeg", "mp4", "m4v", "mov", "webm", "avi", "wmv", "mpg", "mpeg",
                    "ogv", "mp3", "wav", "m4a", "aac", "flac", "aif", "aiff", "caf", "ogg", "oga",
                    "wma",
                ],
            )
            .pick_file()
        {
            self.preview(path, ctx);
        }
    }
    fn preview(&mut self, path: PathBuf, ctx: &egui::Context) {
        self.stop_player();
        self.image = None;
        self.path = path.clone();
        self.status.clear();
        self.open = true;
        let ext = path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if matches!(ext.as_str(), "jpg" | "jpeg" | "png") {
            let (tx, rx) = mpsc::channel();
            self.loading = Some(rx);
            let context = ctx.clone();
            std::thread::spawn(move || {
                let _ = tx.send(image::load(&path));
                context.request_repaint();
            });
        } else {
            let run = || -> io::Result<Child> {
                if !path.metadata()?.is_file() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "通常ファイルを選択してください",
                    ));
                }
                let player = env::current_exe()?
                    .with_file_name(format!("mitaka-media{}", env::consts::EXE_SUFFIX));
                Command::new(player).arg(path).stdin(Stdio::null()).spawn()
            };
            match run() {
                Ok(child) => {
                    self.player = Some(child);
                    self.status = "プレーヤーの再生ボタンで開始します。".into();
                }
                Err(e) => self.status = format!("メディアを開けません: {e}"),
            }
        }
    }
    pub fn frame(&mut self, ctx: &egui::Context) {
        if let Some(rx) = &self.loading {
            match rx.try_recv() {
                Ok(result) => {
                    self.loading = None;
                    match result {
                        Ok(image) if self.open => {
                            self.image = Some(image::Preview::new(ctx, image))
                        }
                        Ok(_) => {}
                        Err(e) => self.status = e.to_string(),
                    }
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.loading = None;
                    self.status = "画像の読み込みが中断しました".into();
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        if let Some(child) = &mut self.player {
            match child.try_wait() {
                Ok(Some(status)) => {
                    self.status = if status.success() {
                        "プレーヤーを閉じました".into()
                    } else {
                        format!("プレーヤーの終了コード: {status}")
                    };
                    self.player = None;
                }
                Ok(None) => ctx.request_repaint_after(Duration::from_millis(200)),
                Err(e) => {
                    self.status = e.to_string();
                    self.stop_player();
                }
            }
        }
        let mut open = self.open;
        egui::Window::new("メディアのプレビュー")
            .open(&mut open)
            .default_width(520.)
            .show(ctx, |ui| {
                if self.path != Path::new("") {
                    ui.label(self.path.display().to_string());
                }
                if let Some(image) = &mut self.image {
                    image.show(ui);
                }
                if self.loading.is_some() {
                    ui.spinner();
                }
                if !self.status.is_empty() {
                    ui.label(&self.status);
                }
                if self.player.is_some() && ui.button("プレーヤーを閉じる").clicked() {
                    self.stop_player();
                    self.status = "プレーヤーを閉じました".into();
                }
            });
        if self.open && !open {
            self.stop_player();
            self.image = None;
        }
        self.open = open;
    }
}
impl Drop for Media {
    fn drop(&mut self) {
        self.stop_player();
    }
}
