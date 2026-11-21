mod document;
mod run;

use self::{
    document::{Document, Piece},
    run::{Event, Run},
};
use super::App;
use eframe::egui;
use serde_json::{json, Value};
use std::{
    fs::OpenOptions,
    io::{self, Write},
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Default)]
pub(super) struct Scripts {
    job: Option<Run>,
    document: Option<Document>,
    pub(super) panel: bool,
    log: String,
}
impl Scripts {
    fn append(&mut self, text: &str) {
        self.log.push_str(text);
        self.log.push('\n');
        let mut cut = self.log.len().saturating_sub(1024 * 1024);
        while !self.log.is_char_boundary(cut) {
            cut += 1;
        }
        if cut > 0 {
            self.log.drain(..cut);
        }
    }
    fn launch(
        &mut self,
        file: PathBuf,
        work: PathBuf,
        dir: tempfile::TempDir,
        ctx: &egui::Context,
    ) {
        self.panel = true;
        if self.job.is_some() {
            self.append("実行中のスクリプトを先に停止してください");
            return;
        }
        match Run::start(file, work, dir, ctx) {
            Ok(run) => {
                self.job = Some(run);
                self.append("スクリプトを開始しました");
            }
            Err(e) => self.append(&format!("Lua helper を起動できません: {e}")),
        }
    }
    fn cancel(&mut self) {
        if self.job.take().is_some() {
            self.append("スクリプトを停止しました");
        }
    }
}

fn local_source(dir: &Path, source: &str) -> io::Result<PathBuf> {
    let path = dir.join("html-link.lua");
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut f = options.open(&path)?;
    f.write_all(source.as_bytes())?;
    f.flush()?;
    Ok(path)
}

impl App {
    pub(super) fn script_file(&mut self, ctx: &egui::Context) {
        if self.scripts.job.is_some() {
            self.scripts.panel = true;
            return;
        }
        let Some(path) = rfd::FileDialog::new()
            .set_title("実行するローカル Lua を選択")
            .add_filter("Lua", &["lua"])
            .pick_file()
        else {
            return;
        };
        let work = self
            .data
            .as_ref()
            .and_then(|d| d.root.parent())
            .or(path.parent())
            .unwrap_or(Path::new("."))
            .to_owned();
        match run::scratch() {
            Ok(dir) => self.scripts.launch(path, work, dir, ctx),
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    pub(super) fn open_script_html(&mut self) {
        self.scripts.document = None;
        let Some(path) = rfd::FileDialog::new()
            .set_title("ローカル HTML を開く")
            .add_filter("HTML", &["htm", "html"])
            .pick_file()
        else {
            return;
        };
        match Document::open(path) {
            Ok(document) => self.scripts.document = Some(document),
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    fn script_message(&mut self, msg: &Value) -> Result<Value, String> {
        match msg["op"].as_str() {
            Some("snapshot") => Ok(
                json!({"ok":true,"jd":self.jd,"lines":self.options.lines,"labels":self.options.labels,"timeSpeed":self.playback.plus_speed,"timeStepValue":self.playback.plus_value,"timeStep":self.playback.plus_step,"timeInfo":self.playback.show,"realtime":self.playback.realtime,"planetOrbits":self.solar.orbits,"earthClouds":self.solar.surfaces.clouds,"earthCityLights":self.solar.surfaces.lights,"shadows":self.solar.surfaces.eclipse,"atmosphereMode":self.solar.surfaces.air as u8}),
            ),
            Some("apply") => {
                match msg["kind"].as_u64() {
                    Some(kind @ (0 | 2 | 19 | 20 | 72..=78)) => self.time_action(kind, msg)?,
                    Some(kind @ (29 | 69 | 70 | 79)) => {
                        let flag = msg["enabled"].as_bool().ok_or("真偽値が必要です")?;
                        match kind {
                            29 => self.solar.orbits = flag,
                            69 => self.solar.surfaces.clouds = flag,
                            70 => self.solar.surfaces.lights = flag,
                            _ => self.solar.surfaces.eclipse = flag,
                        }
                    }
                    Some(71) => {
                        let v = msg["number"]
                            .as_f64()
                            .filter(|n| n.fract() == 0.0 && (0. ..=2.).contains(n))
                            .ok_or("大気モードの範囲外です")?;
                        self.solar.surfaces.air =
                            crate::solar::sphere::Air::from_index(v as u64).unwrap();
                    }
                    Some(5) => {
                        self.options.lines = msg["enabled"].as_bool().ok_or("lines は真偽値です")?
                    }
                    Some(7) => {
                        self.options.labels =
                            msg["enabled"].as_bool().ok_or("labels は真偽値です")?
                    }
                    _ => return Err("この画面では未対応の Lua 操作です".into()),
                }
                Ok(json!({"ok":true}))
            }
            Some("log") => {
                let text = msg["text"].as_str().ok_or("出力が文字列ではありません")?;
                self.scripts.append(text);
                Ok(json!({"ok":true}))
            }
            _ => Err("未対応の Lua 要求です".into()),
        }
    }

    pub(super) fn scripts_frame(&mut self, ctx: &egui::Context) {
        while let Some(event) = self
            .scripts
            .job
            .as_ref()
            .and_then(|job| job.events.try_recv().ok())
        {
            match event {
                Event::Request(message, answer) => {
                    let reply = self
                        .script_message(&message)
                        .unwrap_or_else(|e| json!({"ok":false,"error":e}));
                    let _ = answer.send(reply);
                }
                Event::End(status) => {
                    self.scripts.job.take();
                    self.scripts.append(&status.unwrap_or_else(|e| e));
                }
            }
        }
        if let Some(job) = &self.scripts.job {
            if job.deadline_reached() {
                self.scripts.cancel();
            } else {
                ctx.request_repaint_after(Duration::from_millis(40));
            }
        }
        let mut open = self.scripts.panel;
        egui::Window::new("Lua の実行")
            .open(&mut open)
            .show(ctx, |ui| {
                if ui
                    .add_enabled(self.scripts.job.is_some(), egui::Button::new("停止"))
                    .clicked()
                {
                    self.scripts.cancel();
                }
                egui::ScrollArea::vertical()
                    .max_height(300.)
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        ui.monospace(&self.scripts.log);
                    });
            });
        self.scripts.panel = open;
        self.script_document(ctx);
    }

    fn script_document(&mut self, ctx: &egui::Context) {
        let Some(doc) = &mut self.scripts.document else {
            return;
        };
        let mut open = true;
        let mut clicked = None;
        let mut revoked = false;
        egui::Window::new("ローカル HTML")
            .open(&mut open)
            .default_width(540.)
            .show(ctx, |ui| {
                ui.label(doc.path().display().to_string());
                if doc.styles {
                    ui.label("文書のスタイルが表示に影響するため、Lua リンクを無効にしています。");
                } else {
                    ui.checkbox(
                        &mut doc.trusted,
                        "このファイルの Lua リンクを自分の権限で実行する",
                    );
                }
                ui.separator();
                egui::ScrollArea::vertical()
                    .max_height(450.)
                    .show(ui, |ui| {
                        for (bi, block) in doc.blocks.iter().enumerate() {
                            ui.horizontal_wrapped(|ui| {
                                for (pi, part) in block.parts.iter().enumerate() {
                                    match part {
                                        Piece::Text(text) if block.heading => {
                                            ui.heading(text);
                                        }
                                        Piece::Text(text) => {
                                            ui.label(text);
                                        }
                                        Piece::Link {
                                            label,
                                            can_run: true,
                                            ..
                                        } => {
                                            let enabled = doc.trusted && self.scripts.job.is_none();
                                            if ui
                                                .add_enabled(enabled, egui::Link::new(label))
                                                .clicked()
                                            {
                                                clicked = Some((bi, pi));
                                            }
                                        }
                                        Piece::Link { label, .. } => {
                                            if ui
                                                .link(label)
                                                .on_hover_text("このリンクは移動・実行できません")
                                                .clicked()
                                            {
                                                revoked = true;
                                            }
                                        }
                                    }
                                }
                            });
                        }
                    });
            });
        if !open {
            self.scripts.document = None;
            return;
        }
        if revoked {
            doc.trusted = false;
            return;
        }
        if let Some((b, p)) = clicked {
            let source = match doc.activate(b, p) {
                Ok(v) => v,
                Err(e) => {
                    self.error = Some(e.to_string());
                    return;
                }
            };
            let work = self
                .data
                .as_ref()
                .and_then(|d| d.root.parent())
                .or(doc.path().parent())
                .unwrap_or(Path::new("."))
                .to_owned();
            let result = run::scratch().and_then(|dir| {
                let file = local_source(dir.path(), &source)?;
                Ok((dir, file))
            });
            match result {
                Ok((dir, file)) => self.scripts.launch(file, work, dir, ctx),
                Err(e) => self.error = Some(e.to_string()),
            }
        }
    }
}
