mod document;
mod run;

use self::{
    document::{Document, Tag},
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
    input: String,
    history: std::collections::VecDeque<String>,
    cursor: Option<usize>,
    draft: String,
    composing: bool,
}
const CONSOLE_BYTES: usize = 1024 * 1024;

impl Scripts {
    fn stage_console(&self) -> io::Result<(tempfile::TempDir, PathBuf)> {
        if self.input.len() > CONSOLE_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "入力は UTF-8 で 1 MiB 以下にしてください",
            ));
        }
        let dir = run::scratch()?;
        let path = local_source(dir.path(), &self.input)?;
        Ok((dir, path))
    }

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
        console: bool,
        work: PathBuf,
        dir: tempfile::TempDir,
        ctx: &egui::Context,
    ) {
        self.panel = true;
        if self.job.is_some() {
            self.append("実行中のスクリプトを先に停止してください");
            return;
        }
        match Run::start(file, console, work, dir, ctx) {
            Ok(run) => {
                self.job = Some(run);
                self.append("スクリプトを開始しました");
            }
            Err(e) => self.append(&format!("Lua helper を起動できません: {e}")),
        }
    }
    pub(super) fn start_switch_probe(&mut self, ctx: &egui::Context) -> io::Result<()> {
        let dir = run::scratch()?;
        let path = local_source(dir.path(), "Time.JD = Time.JD + 1")?;
        let job = Run::start(path, false, dir.path().to_owned(), dir, ctx)?;
        // 実ヘルパーがJDを取得できた時点で、次の書き換え要求はUIからの応答を待つ。
        match job.events.recv_timeout(Duration::from_secs(10)) {
            Ok(Event::Request(msg, reply)) if msg["op"] == "snapshot" => {
                reply
                    .send(json!({"ok": true, "jd": 2451545.0}))
                    .map_err(io::Error::other)?;
            }
            _ => return Err(io::Error::other("検証用Luaの日時読取要求が到着しません")),
        }
        self.job = Some(job);
        Ok(())
    }
    pub(super) fn cancel(&mut self) {
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
            Ok(dir) => self.scripts.launch(path, false, work, dir, ctx),
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

    fn script_message(&mut self, msg: &Value, ctx: &egui::Context) -> Result<Value, String> {
        match msg["op"].as_str() {
            Some("constellations") => {
                let data = self.data.as_ref().ok_or("天文データがありません")?;
                self.options
                    .layers
                    .constellation_request(&data.constellations, msg)
            }
            Some("media") => self.media.script(msg, ctx),
            Some("snapshot") => {
                let mut reply = self.camera_snapshot(self.options.layers.snapshot(json!({
                "ok":true,
                "jd":self.jd,
                "targetSelectionMode":self.picking as u8,
                "plusTimeZoneWestSeconds":self.session.0["plus_time_zone"]["seconds_west_utc"],
                "lines":self.options.lines,
                "labels":self.options.labels,
                "timeSpeed":self.playback.plus_speed,
                "timeStepValue":self.playback.plus_value,
                "timeStep":self.playback.plus_step,
                "timeInfo":self.playback.show,
                "realtime":self.playback.realtime,
                "planetOrbits":self.solar.orbits,
                "selectedStarNames":self.options.layers.star_selection(self.data.as_ref().map(|d|&d.layers),msg["starHIP"].as_i64()),
                "radiusScale":self.solar.layers.scales,
                "radiusScaleMode":u8::from(!self.solar.layers.keep_radius),
                "planetsVisible":self.solar.layers.visible,
                "planetLabels":self.solar.layers.labels,
                "planetAxes":self.solar.layers.axes,
                "topoAmplification":self.terrain_amplification.stage(),
                "earthClouds":self.solar.surfaces.clouds,
                "earthCityLights":self.solar.surfaces.lights,
                "shadows":self.solar.surfaces.eclipse.shadows,
                "shadowBorders":self.solar.surfaces.eclipse.borders,
                "shadowUmbra":self.solar.surfaces.eclipse.umbra,
                "shadowPenumbra":self.solar.surfaces.eclipse.penumbra,
                "atmosphereMode":self.solar.surfaces.air as u8,
                "fullScreen":self.fullscreen.actual(ctx),
                "targetBrowser":self.objects.open,
                "timeSetting":self.time_settings,
                "placeSetting":self.place_settings,
                "scriptOutput":self.scripts.panel,
                "dualScreen":self.projector.open,
            })));
                self.objects.display.snapshot(&mut reply);
                Ok(reply)
            }
            Some("time_animation") => {
                self.animate_time(msg)?;
                ctx.request_repaint();
                Ok(json!({"ok":true}))
            }
            Some("apply") => {
                match msg["kind"].as_u64() {
                    Some(64) => {
                        self.data.as_ref().ok_or("天文データがありません")?;
                        self.options.layers.animate_brightness(
                            msg["number"].as_f64().ok_or("明るさが必要です")?,
                            msg["duration"].as_f64().ok_or("秒数が必要です")?,
                            std::time::Instant::now(),
                        )?;
                        ctx.request_repaint();
                    }
                    Some(kind @ (67 | 68)) => {
                        let data = self.data.as_ref().ok_or("天文データがありません")?;
                        let hips = if kind == 67 {
                            Some(
                                msg["hips"]
                                    .as_array()
                                    .ok_or("HIP一覧が必要です")?
                                    .as_slice(),
                            )
                        } else {
                            None
                        };
                        self.options.layers.select_stars(
                            &data.layers,
                            hips,
                            msg["enabled"].as_bool().ok_or("真偽値が必要です")?,
                        )?;
                    }
                    Some(kind @ (87 | 88)) => self.solar.layers.scale_action(kind, msg)?,
                    Some(kind @ (3 | 4 | 12 | 89..=105)) => self.camera_action(kind, msg, ctx)?,
                    Some(kind @ (0 | 2 | 19 | 20 | 72..=78)) => self.time_action(kind, msg)?,
                    Some(kind @ (27..=30 | 69 | 70 | 79..=82)) => {
                        let flag = msg["enabled"].as_bool().ok_or("真偽値が必要です")?;
                        match kind {
                            27 => self.solar.layers.visible = flag,
                            28 => self.solar.layers.labels = flag,
                            29 => self.solar.orbits = flag,
                            30 => self.solar.layers.axes = flag,
                            69 => self.solar.surfaces.clouds = flag,
                            70 => self.solar.surfaces.lights = flag,
                            79 => self.solar.surfaces.eclipse.shadows = flag,
                            80 => self.solar.surfaces.eclipse.borders = flag,
                            81 => self.solar.surfaces.eclipse.umbra = flag,
                            _ => self.solar.surfaces.eclipse.penumbra = flag,
                        }
                    }
                    Some(kind @ (13..=17 | 41)) => {
                        let enabled = msg["enabled"].as_bool().ok_or("真偽値が必要です")?;
                        match kind {
                            13 => self.objects.open = enabled,
                            14 => self.time_settings = enabled,
                            15 => self.place_settings = enabled,
                            16 => self.scripts.panel = enabled,
                            17 => self.projector.open = enabled,
                            _ => self.fullscreen.request(ctx, Some(enabled))?,
                        }
                    }
                    Some(kind @ 44..=59) => self
                        .objects
                        .display
                        .apply(kind, msg["enabled"].as_bool().ok_or("真偽値が必要です")?),
                    Some(106) => {
                        self.picking = msg["number"]
                            .as_f64()
                            .filter(|n| n.fract() == 0. && (0. ..=2.).contains(n))
                            .and_then(|n| crate::viewer::Picking::from_index(n as u64))
                            .ok_or("選択モードの範囲外です")?;
                    }
                    Some(18) => self.execute(crate::commands::Command::Export, ctx),
                    Some(42) => self.fullscreen.request(ctx, None)?,
                    Some(43) => self.execute(crate::commands::Command::About, ctx),
                    Some(86) => {
                        let raw = msg["number"]
                            .as_f64()
                            .filter(|v| v.fract() == 0. && (1. ..=5.).contains(v))
                            .ok_or("地形倍率は1から5の整数段階です")?;
                        let mode = mitaka_core::terrain::Amplification::from_stage(raw as u64)
                            .map_err(|e| e.to_string())?;
                        self.set_terrain_mode(mode).map_err(|e| e.to_string())?;
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
                    Some(kind) if self.options.layers.action(kind, msg)? => {}
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
                        .script_message(&message, ctx)
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
        let mut run_console = false;
        egui::Window::new("Lua の実行")
            .open(&mut open)
            .show(ctx, |ui| {
                let id = ui.make_persistent_id("lua-command");
                let shortcut = if ui.memory(|m| m.has_focus(id)) {
                    crate::shortcuts::console(ui.ctx(), &mut self.scripts.composing)
                } else {
                    self.scripts.composing = false;
                    None
                };
                ui.add(
                    egui::TextEdit::multiline(&mut self.scripts.input)
                        .id(id)
                        .hint_text("Lua の式または文")
                        .code_editor()
                        .desired_rows(3),
                );
                ui.horizontal(|ui| {
                    use crate::shortcuts::ConsoleKey;
                    let can_run =
                        self.scripts.job.is_none() && !self.scripts.input.trim().is_empty();
                    run_console = ui.add_enabled(can_run, egui::Button::new("実行")).clicked()
                        || (can_run && shortcut == Some(ConsoleKey::Run));
                    let can_go_back =
                        !self.scripts.history.is_empty() && self.scripts.cursor != Some(0);
                    if ui
                        .add_enabled(can_go_back, egui::Button::new("前の入力"))
                        .clicked()
                        || (can_go_back && shortcut == Some(ConsoleKey::Previous))
                    {
                        if self.scripts.cursor.is_none() {
                            self.scripts.draft = self.scripts.input.clone();
                        }
                        let i = self.scripts.cursor.unwrap_or(self.scripts.history.len()) - 1;
                        self.scripts.cursor = Some(i);
                        self.scripts.input = self.scripts.history[i].clone();
                    }
                    let can_go_forward = self.scripts.cursor.is_some();
                    if ui
                        .add_enabled(can_go_forward, egui::Button::new("次の入力"))
                        .clicked()
                        || (can_go_forward && shortcut == Some(ConsoleKey::Next))
                    {
                        let i = self.scripts.cursor.unwrap() + 1;
                        self.scripts.cursor = (i < self.scripts.history.len()).then_some(i);
                        self.scripts.input = self
                            .scripts
                            .history
                            .get(i)
                            .unwrap_or(&self.scripts.draft)
                            .clone();
                    }
                });
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
        if run_console {
            let work = self
                .data
                .as_ref()
                .and_then(|d| d.root.parent())
                .unwrap_or(Path::new("."))
                .to_owned();
            match self.scripts.stage_console() {
                Ok((dir, path)) => {
                    self.scripts.launch(path, true, work, dir, ctx);
                    if self.scripts.job.is_some() {
                        let code = std::mem::take(&mut self.scripts.input);
                        if self.scripts.history.back() != Some(&code) {
                            if self.scripts.history.len() == 64 {
                                self.scripts.history.pop_front();
                            }
                            self.scripts.history.push_back(code);
                        }
                        self.scripts.cursor = None;
                        self.scripts.draft.clear();
                    }
                }
                Err(e) => self.scripts.append(&e.to_string()),
            }
        }
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
                    ui.label("この文書には表示を確認できない指定があるため、Lua リンクを無効にしています。");
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
                        match doc.draw(ui,self.scripts.job.is_none()) {
                            Some(Tag::Action(bi,pi))=>clicked=Some((bi,pi)),
                            Some(Tag::Revoke)=>revoked=true,
                            None=>{},
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
                Ok((dir, file)) => self.scripts.launch(file, false, work, dir, ctx),
                Err(e) => self.error = Some(e.to_string()),
            }
        }
    }
}
impl App {
    pub(crate) fn start_camera_check(&mut self, ctx: &egui::Context) -> io::Result<()> {
        let data = self
            .data
            .as_ref()
            .ok_or_else(|| io::Error::other("天文データがありません"))?;
        let root = data.root.clone();
        // 2000年1月・3月を使う繰越検査は、暦表が両方の検査日時を含むときに行う。
        let [first, last] = data.date_range;
        let check_2000_dates = first <= crate::J2000 && last >= crate::J2000 + 60.;
        let check_month_end =
            first <= 2460341.5 - 0.25 / 86400. && last >= 2460372.5 - 0.25 / 86400.;
        self.objects = Default::default();
        self.solar = Default::default();
        self.view = crate::projection::View::Solar;
        let dir = run::scratch()?;
        let check = r#"
GUI.timeSetting = true
GUI.placeSetting = true
GUI:togglePlaceSetting()
assert(GUI.timeSetting and not GUI.placeSetting)
GUI.placeSetting = true
GUI:toggleTimeSetting()
assert(GUI.placeSetting and not GUI.timeSetting)
GUI.placeSetting = false
Asteroids.selectedOnly=true
Satellites.orbits=true
assert(Asteroids.selectedOnly and Satellites.orbits)
Satellites.orbits=false
Asteroids.selectedOnly=false
Targets.selectMode = 1
assert(Targets.selectMode == 1)
Targets.selectMode = 2
Grids.squareScales=true
assert(Grids.squareScales)
Renderer.textSmooth=false
Renderer.lineSmooth=false
Renderer.pointSmooth=false
assert(not Renderer.textSmooth and not Renderer.lineSmooth)
local saved = Time.JD
if check_month_end then
    local step = Time.step
    Time.JD = 2460341.5 - 0.25 / 86400
    Time.step = STEP_MONTH
    Time:forward()
    assert(math.abs(Time.JD - (2460372.5 - 0.25 / 86400)) < 1e-9)
    Time.step = step
end
if check_2000_dates then
    Time:setTime(2000,2,30,3,4,5)
    assert(Time:getTimeString() == '2000,3,1,3,4,5')
    Time.min = 61
    assert(Time.hour == 4 and Time.minute == 1)
    local date = '0x7d0,01,02,03,04,05suffix'
    assert(Time:setTimeString(date) == date and Time.day == 2)
end
assert(not pcall(function() return Time.localYear end))
Time:setJDTo(saved, 0, false)
assert(Time.JD == saved)
Camera.target='Satellite.Moon'
Camera:forceMode(0); assert(Camera.mode==0)
local landed_zoom = Camera.zoom
local x0,y0,z0,w0 = Camera:getOrient()
assert(not pcall(function() Camera.zoom=2*UNIT_AU end))
assert(not pcall(function() Camera:setZoomTo(2*UNIT_AU,1) end))
assert(not pcall(function() Camera:setZoomLogTo(2*UNIT_AU,1) end))
local x1,y1,z1,w1 = Camera:getOrient()
assert(Camera.zoom == landed_zoom and x0==x1 and y0==y1 and z0==z1 and w0==w1)
Camera:setFovTo(50,0); assert(Camera.fov==50)
Camera:setAzimuthTo(28,0);Camera:setAltitudeTo(-9,0)
assert(Camera.mode==0 and math.abs(Camera.azimuth-28)<1e-8 and math.abs(Camera.altitude+9)<1e-8)
Camera:setModeTo(1,0);assert(Camera.mode==1)
Camera:setModeTo(0,0);assert(Camera.mode==0)
Camera:forceMode(1);assert(Camera.mode==1)
Camera.target='Planet.Earth'
assert(Camera.target=='Planet.Earth')
Camera.duration=0
Camera:setZoomTo(2*UNIT_AU)
assert(math.abs(Camera.zoom / UNIT_AU-2)<1e-10)
Camera:setZoomLogTo(3*UNIT_AU,0)
assert(math.abs(Camera.zoom / UNIT_AU-3)<1e-10)
Camera:setEuler(15,22,-7)
local h,p,r=Camera:getEuler()
assert(math.abs(h-15)<1e-8 and math.abs(p-22)<1e-8 and math.abs(r+7)<1e-8)
local x,y,z,w=Camera:getOrient()
Camera:setOrientTo(x,y,z,w,0,false)
Camera:setAzimuthTo(-60,0); Camera:setAltitudeTo(36,0,false)
assert(Camera.azimuth==-60 and Camera.altitude==36)
Camera:setFovTo(45,0);assert(Camera.fov==45)
Camera:lookTo('Star.Sirius',0)
assert(not pcall(function() Camera:setOrient(0,0,0,0) end))
assert(not pcall(function() Camera:setZoomLogTo(-1,1) end))
local previousBrightness = Stars.brightness
Stars:setBrightnessTo(0.25,0)
assert(Stars.brightness==0.25)
Stars.brightness=previousBrightness
Stars:select('All')
if Stars:isSelected('32349') then
    Stars:deselect('32349 999999')
    assert(not Stars:isSelected('32349'))
    Stars:select('32349')
    assert(Stars:isSelected('32349'))
end
assert(not pcall(function() Stars:select('32349 -1') end))
Constels:show('All')
if Constels:isVisible('Ori') then
    Constels:hide('All')
    Constels:show('Ori')
    assert(Constels:isVisible('Ori'))
    Constels:hide('Ori')
    assert(not Constels:isVisible('Ori'))
end
Constels:show('All')
Planets:setScaleMode(mtk.KEEP_ORBIT)
Planets:setScale(2,4,3,5)
local a,b,c,d=Planets:getScale()
assert(a==2 and b==4 and c==3 and d==5)
Planets.scaleMode=mtk.KEEP_RADIUS
assert(Planets:getScaleMode()==mtk.KEEP_RADIUS)
assert(not pcall(function() Planets:setScale(3,4,1,1) end))
a,b,c,d=Planets:getScale()
assert(a==2 and b==4 and c==3 and d==5)
Planets:setScaleMode(mtk.KEEP_ORBIT)
Planets:setScale(1,1,1,1)
print('camera-check-complete')
"#;
        let check = if root.join("star_names_HIP.dat").is_file() {
            std::borrow::Cow::Borrowed(check)
        } else {
            std::borrow::Cow::Owned(check.replace("Star.Sirius", "Planet.Mars"))
        };
        let check = format!("local check_2000_dates, check_month_end = {check_2000_dates}, {check_month_end}\n{check}");
        let file = local_source(dir.path(), &check)?;
        self.scripts.launch(file, false, root, dir, ctx);
        Ok(())
    }
    pub(crate) fn camera_check_complete(&self) -> io::Result<bool> {
        if self.scripts.job.is_some() {
            return Ok(false);
        }
        if self.scripts.log.contains("camera-check-complete") {
            Ok(true)
        } else {
            Err(io::Error::other(format!(
                "Lua camera test: {}",
                self.scripts.log
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn console_stages_a_mebibyte_and_preserves_oversized_input() {
        let mut scripts = Scripts {
            input: "月".repeat(CONSOLE_BYTES / 3),
            ..Default::default()
        };
        scripts.input.push(' ');
        assert_eq!(scripts.input.len(), CONSOLE_BYTES);
        let (_dir, path) = scripts.stage_console().unwrap();
        assert_eq!(std::fs::read_to_string(path).unwrap(), scripts.input);
        scripts.input.push('!');
        let original = scripts.input.clone();
        assert_eq!(
            scripts.stage_console().unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
        assert_eq!(scripts.input, original);
        assert!(scripts.job.is_none() && scripts.history.is_empty());
    }
}
