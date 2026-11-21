mod camera_control;
mod capture;
mod catalog;
mod chart;
mod commands;
mod deep_sky;
mod fonts;
mod galaxies;
mod layers;
mod media_preview;
mod objects;
mod planet_layers;
mod planet_scale;
mod playback;
mod projection;
mod projector;
mod script_ui;
mod session;
mod session_ui;
mod smoke;
mod solar;
mod surface;
mod viewer;

use crate::{
    catalog::SkyData,
    chart::{Chart, Options},
    commands::{Command, Menus},
    projection::{Camera, View},
};
use chrono::{DateTime, NaiveDateTime, Utc};
use eframe::egui::{self, Color32, Key};
use mitaka_core::astronomy::ObservationFrame;
use std::{env, error::Error, ffi::OsString, path::PathBuf};

type AppError = Box<dyn Error + Send + Sync>;
const J2000: f64 = 2451545.;
const UNIX_EPOCH_JD: f64 = 2440587.5;
fn utc_date(jd: f64) -> Option<DateTime<Utc>> {
    DateTime::from_timestamp(((jd - UNIX_EPOCH_JD) * 86400.).round() as i64, 0)
}
fn parse_utc(text: &str) -> Option<f64> {
    let date = NaiveDateTime::parse_from_str(text.trim(), "%Y-%m-%d %H:%M:%S").ok()?;
    Some(UNIX_EPOCH_JD + date.and_utc().timestamp() as f64 / 86400.)
}

#[derive(Default)]
struct Args {
    data: Option<PathBuf>,
    inspect: bool,
    smoke: bool,
    help: bool,
}
impl Args {
    fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Self, String> {
        let mut result = Self::default();
        let mut iter = args.into_iter();
        while let Some(arg) = iter.next() {
            if arg == "--data-root" {
                if result.data.is_some() {
                    return Err("--data-root が重複しています".into());
                }
                let value = iter
                    .next()
                    .filter(|p| !p.is_empty() && !p.to_string_lossy().starts_with("--"))
                    .ok_or("--data-root の後に data ディレクトリを指定してください")?;
                result.data = Some(PathBuf::from(value));
            } else if arg == "--inspect" {
                result.inspect = true;
            } else if arg == "--smoke-test" {
                result.smoke = true;
            } else if arg == "--help" || arg == "-h" {
                result.help = true;
            } else {
                return Err(format!("不明な引数 {}", arg.to_string_lossy()));
            }
        }
        if result.inspect && result.smoke {
            return Err("--inspect と --smoke-test は個別に指定してください".into());
        }
        Ok(result)
    }
}
struct App {
    picking: viewer::Picking,
    quality: viewer::Quality,
    motion: camera_control::State,
    terrain_amplification: mitaka_core::terrain::Amplification,
    background_render: layers::background::Renderer,
    landscape_render: layers::landscape::LandscapeRenderer,
    projector: projector::Projector,
    media: media_preview::Media,
    scripts: script_ui::Scripts,
    playback: playback::Playback,
    session: session::Session,
    data: Option<SkyData>,
    solar: solar::Solar,
    surface: Option<surface::Surface>,
    galaxies: Option<galaxies::Browser>,
    capture: Option<PathBuf>,
    canvas_rect: egui::Rect,
    objects: objects::Browser,
    menus: Menus,
    view: View,
    sky: Camera,
    horizon: Camera,
    options: Options,
    jd: f64,
    latitude: f64,
    longitude: f64,
    selected: Option<i32>,
    query: String,
    search: bool,
    settings: bool,
    info: bool,
    about: bool,
    search_message: String,
    error: Option<String>,
    utc: String,
    smoke: Option<smoke::Smoke>,
}
impl App {
    fn new(
        cc: &eframe::CreationContext<'_>,
        initial: Option<SkyData>,
        error: Option<String>,
        smoke: Option<smoke::Smoke>,
    ) -> Result<Self, AppError> {
        fonts::install(&cc.egui_ctx);
        let menus = Menus::new(cc)?;
        let mut media = media_preview::Media::default();
        if let Some(data) = &initial {
            media.reset(&data.root);
        }
        Ok(Self {
            picking: Default::default(),
            quality: Default::default(),
            terrain_amplification: Default::default(),
            data: initial,
            motion: Default::default(),
            background_render: Default::default(),
            landscape_render: Default::default(),
            projector: Default::default(),
            media,
            scripts: Default::default(),
            playback: Default::default(),
            session: Default::default(),
            solar: Default::default(),
            surface: None,
            galaxies: None,
            capture: None,
            canvas_rect: egui::Rect::ZERO,
            objects: Default::default(),
            menus,
            view: View::Sky,
            sky: Camera::default(),
            horizon: Camera::default(),
            options: Options::default(),
            jd: J2000,
            latitude: 35.68,
            longitude: 139.76,
            selected: None,
            query: String::new(),
            search: false,
            settings: false,
            info: false,
            about: false,
            search_message: String::new(),
            error,
            utc: "2000-01-01 12:00:00".into(),
            smoke,
        })
    }
    fn set_terrain_mode(
        &mut self,
        mode: mitaka_core::terrain::Amplification,
    ) -> std::io::Result<()> {
        if let Some(surface) = &mut self.surface {
            surface.set_mode(mode)?;
        }
        self.terrain_amplification = mode;
        Ok(())
    }
    fn camera(&mut self) -> &mut Camera {
        if self.view == View::Sky {
            &mut self.sky
        } else {
            &mut self.horizon
        }
    }
    fn set_date(&mut self, jd: f64) {
        match self.apply_date(jd) {
            Ok(()) => self.playback.stop(),
            Err(e) => self.error = Some(e),
        }
    }
    fn open(&mut self) {
        if let Some(root) = rfd::FileDialog::new()
            .set_title("hip.dat と cf96.dat のあるフォルダを開く")
            .pick_folder()
        {
            match SkyData::open(&root) {
                Ok(data) => {
                    if data.validate_date(self.jd).is_err() {
                        self.set_date(J2000);
                    }
                    self.options.layers.observer.clear();
                    self.options.layers.selected = None;
                    self.options.layers.exceptions.clear();
                    self.media.reset(&data.root);
                    self.data = Some(data);
                    self.motion = Default::default();
                    self.objects = Default::default();
                    self.solar = Default::default();
                    self.surface = None;
                    self.galaxies = None;
                    if matches!(self.view, View::Surface) {
                        self.view = View::Sky;
                    }
                    self.selected = None;
                    self.error = None;
                    self.search_message.clear();
                }
                Err(err) => self.error = Some(format!("{}\n{err}", root.display())),
            }
        }
    }
    fn execute(&mut self, command: Command, ctx: &egui::Context) {
        if matches!(
            command,
            Command::Reset
                | Command::Land
                | Command::Sky
                | Command::Horizon
                | Command::SolarControls
                | Command::Surface
        ) {
            self.interrupt_journey();
        }
        match command {
            Command::Open => self.open(),
            Command::Media => self.media.choose(ctx),
            Command::MediaList => self.media.browser = true,
            Command::Projector => {
                self.projector.controls = true;
                self.projector.open = true;
            }
            Command::Fullscreen => ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(
                !ctx.input(|i| i.viewport().fullscreen.unwrap_or(false)),
            )),
            Command::About => self.about = true,
            Command::RunScript => self.script_file(ctx),
            Command::ScriptPanel => self.scripts.panel = true,
            Command::LocalHtml => self.open_script_html(),
            Command::RememberState | Command::RecallState => {
                if let Err(e) = self.remembered_session(command == Command::RememberState) {
                    self.error = Some(e.to_string());
                }
            }
            Command::SaveWindow | Command::RecallWindow => {
                if let Err(e) = viewer::window_position(ctx, command == Command::SaveWindow) {
                    self.error = Some(e.to_string());
                }
            }
            Command::LoadState => self.session_dialog(false),
            Command::SaveState => self.session_dialog(true),
            Command::Sky => self.view = View::Sky,
            Command::Horizon => self.view = View::Horizon,
            Command::Objects => self.objects.open = true,
            Command::Settings => self.settings = true,
            Command::Search => self.search = true,
            Command::Galaxy => {
                self.view = View::Solar;
                if let Some(data) = &self.data {
                    let browser = self
                        .galaxies
                        .get_or_insert_with(|| galaxies::Browser::new(data.root.clone(), ctx));
                    self.solar.stop();
                    self.motion = Default::default();
                    browser.open(&mut self.solar.camera, self.canvas_rect);
                }
            }
            Command::Land => {
                let Some(data) = self.data.as_ref() else {
                    return;
                };
                let ascend = matches!(self.view, View::Horizon | View::Surface)
                    || self
                        .motion
                        .site
                        .is_some_and(|s| self.view == View::Solar && s.height <= 0.002000001);
                let body = match self.view {
                    View::Solar => self.solar.selected,
                    View::Horizon => 3,
                    View::Surface => self.surface.as_ref().map_or(3, |s| s.body),
                    _ => {
                        self.error = Some("地平図または太陽系で惑星・月を選択してください".into());
                        return;
                    }
                };
                let (lat, lon) = if self.view == View::Surface {
                    self.surface
                        .as_ref()
                        .map_or((self.latitude, self.longitude), |s| (s.lat, s.lon))
                } else {
                    (self.latitude, self.longitude)
                };
                if !matches!(body, 3 | 4) {
                    if let Err(e) = self.nonterrain_mode(ascend, 3., false) {
                        self.error = Some(e.to_string());
                    }
                    return;
                }
                let result = (|| -> std::io::Result<()> {
                    let ground = surface::Surface::load_mode(
                        &data.root,
                        body,
                        lat,
                        lon,
                        100.,
                        self.terrain_amplification,
                    )?;
                    self.solar.prepare(&data.ephemeris, self.jd)?;
                    self.solar.begin_surface_flight(
                        body,
                        lat,
                        lon,
                        ground.landing_height_m(solar::RADII_KM[body]),
                        self.jd,
                        ascend,
                    )?;
                    self.motion.site = None;
                    self.surface = Some(ground);
                    self.view = View::Solar;
                    self.solar.show_controls = false;
                    Ok(())
                })();
                if let Err(e) = result {
                    self.error = Some(e.to_string());
                }
            }
            Command::Solar => self.view = View::Solar,
            Command::Journey => self.open_journey(),
            Command::CameraControls => self.motion.controls = true,
            Command::SolarControls => {
                self.view = View::Solar;
                self.solar.show_controls = true;
            }
            Command::Surface => {
                let Some(data) = &self.data else {
                    self.error = Some("天文データを開いてください".into());
                    return;
                };
                let body = if self.view == View::Solar {
                    self.solar.selected
                } else {
                    3
                };
                match surface::Surface::load_mode(
                    &data.root,
                    body,
                    self.latitude,
                    self.longitude,
                    100.,
                    self.terrain_amplification,
                ) {
                    Ok(surface) => {
                        self.surface = Some(surface);
                        self.view = View::Surface;
                    }
                    Err(e) => self.error = Some(e.to_string()),
                }
            }
            Command::Export => {
                if self.data.is_some() && self.capture.is_none() {
                    self.capture = rfd::FileDialog::new()
                        .set_title("表示を画像に保存")
                        .set_file_name("Mitaka.png")
                        .add_filter("PNG", &["png"])
                        .add_filter("JPEG", &["jpg", "jpeg"])
                        .add_filter("BMP", &["bmp"])
                        .save_file();
                }
            }
            Command::Info if self.view == View::Solar => self.solar.show_controls = true,
            Command::Info => self.info = true,
            Command::Reset if self.view == View::Solar => self.solar = Default::default(),
            Command::Reset if self.view == View::Surface => self.view = View::Solar,
            Command::Reset => *self.camera() = Camera::default(),
        }
    }
    fn search_star(&mut self) {
        self.search_message.clear();
        let trimmed = self.query.trim();
        let number = trimmed
            .get(..3)
            .filter(|p| p.eq_ignore_ascii_case("HIP"))
            .map_or(trimmed, |_| trimmed[3..].trim());
        let Some(hip) = number.parse::<i32>().ok().filter(|id| *id > 0) else {
            self.search_message = "HIP 番号を入力してください".into();
            return;
        };
        let Some(data) = &self.data else {
            self.search_message = "データを開いてください".into();
            return;
        };
        let Some(&index) = data.by_hip.get(&hip) else {
            self.search_message = format!("HIP {hip} は見つかりません");
            return;
        };
        let star = &data.stars[index];
        let mut coords = [f64::from(star.ra_deg), f64::from(star.dec_deg)];
        if self.view == View::Horizon {
            match ObservationFrame::new(self.latitude, self.longitude, self.jd)
                .and_then(|frame| frame.to_horizontal(coords[0], coords[1]))
            {
                Ok(horizontal) => coords = horizontal,
                Err(err) => {
                    self.search_message = err.to_string();
                    return;
                }
            }
        }
        self.options.magnitude = self.options.magnitude.max(star.magnitude);
        self.selected = Some(hip);
        self.solar.layers.select(self.view, None);
        if matches!(self.view, View::Solar | View::Surface) {
            self.view = View::Sky;
        }
        let view = self.view;
        self.camera().center(view, coords);
        self.search_message = format!("HIP {hip} を選択しました");
    }
    fn capture_frame(&mut self, ctx: &egui::Context) {
        if self.capture.is_none() {
            return;
        }
        let image = ctx.input(|i| {
            i.events.iter().find_map(|e| {
                if let egui::Event::Screenshot {
                    viewport_id, image, ..
                } = e
                {
                    if *viewport_id != egui::ViewportId::ROOT {
                        return None;
                    }
                    Some(image.clone())
                } else {
                    None
                }
            })
        });
        if let Some(image) = image {
            if let (Some(path), Some(data)) = (self.capture.take(), self.data.as_ref()) {
                if let Err(e) = capture::save(
                    &image,
                    &path,
                    &data.root,
                    self.canvas_rect,
                    ctx.pixels_per_point(),
                ) {
                    self.error = Some(e.to_string());
                }
            }
        } else {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
            ctx.request_repaint();
        }
    }
    fn panels(&mut self, ctx: &egui::Context) {
        egui::Window::new("Mitaka Plus RS について")
            .open(&mut self.about)
            .show(ctx, |ui| {
                ui.heading(concat!("Mitaka Plus RS ", env!("CARGO_PKG_VERSION")));
                ui.label(
                    "国立天文台4D2UプロジェクトのMitakaとMitaka Plusをもとにした非公式Rust移植版。",
                );
                ui.label("天文データ・画像の権利は各制作者に帰属します。");
            });
        self.projector.settings(ctx);
        self.camera_controls(ctx);
        self.journey_panel(ctx);
        self.object_panels(ctx);
        self.solar.panel(ctx);
        if let Some(g) = &mut self.galaxies {
            if self.view == View::Solar {
                g.panel(
                    ctx,
                    &mut self.solar.camera,
                    self.canvas_rect,
                    &mut self.options.layers.deep_sky,
                );
            }
        }
        let mut search = self.search;
        egui::Window::new("恒星を検索")
            .open(&mut search)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("HIP 番号");
                    let edit = ui.text_edit_singleline(&mut self.query);
                    if edit.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter))
                        || ui.button("検索").clicked()
                    {
                        self.search_star();
                    }
                });
                if !self.search_message.is_empty() {
                    ui.label(&self.search_message);
                }
            });
        self.search = search;
        let mut settings = self.settings;
        egui::Window::new("観測・表示設定")
            .open(&mut settings)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("UTC");
                    ui.text_edit_singleline(&mut self.utc);
                    if ui.button("適用").clicked() {
                        if let Some(jd) = parse_utc(&self.utc) {
                            self.set_date(jd);
                        } else {
                            self.error =
                                Some("UTC は YYYY-MM-DD HH:MM:SS で指定してください".into());
                        }
                    }
                });
                ui.horizontal(|ui| {
                    if ui.button("前日").clicked() {
                        self.set_date(self.jd - 1.);
                    }
                    if ui.button("翌日").clicked() {
                        self.set_date(self.jd + 1.);
                    }
                    if ui.button("現在").clicked() {
                        self.set_date(UNIX_EPOCH_JD + Utc::now().timestamp() as f64 / 86400.);
                    }
                    if ui.button("J2000").clicked() {
                        self.set_date(J2000);
                    }
                    ui.label(format!("JD {:.6}", self.jd));
                });
                ui.separator();
                self.clock_controls(ui);
                self.solar.layers.panel(ui);
                let mut mode = self.terrain_amplification;
                egui::ComboBox::from_label("地形の起伏倍率")
                    .selected_text(format!("×{}", mode.factor()))
                    .show_ui(ui, |ui| {
                        for value in mitaka_core::terrain::Amplification::ALL {
                            ui.selectable_value(&mut mode, value, format!("×{}", value.factor()));
                        }
                    });
                if let Err(e) = self.set_terrain_mode(mode) {
                    self.error = Some(e.to_string());
                }
                let lat_changed = ui
                    .add(egui::Slider::new(&mut self.latitude, -90. ..=90.).text("緯度 °"))
                    .changed();
                let lon_changed = ui
                    .add(egui::Slider::new(&mut self.longitude, -180. ..=180.).text("東経 °"))
                    .changed();
                if lat_changed || lon_changed {
                    self.options.layers.observer.clear();
                }
                self.picking.ui(ui);
                self.objects.display.ui(ui);
                self.options.layers.ui(
                    ui,
                    self.data.as_ref(),
                    &mut self.latitude,
                    &mut self.longitude,
                );
                ui.add(
                    egui::Slider::new(&mut self.options.magnitude, -1.5..=16.).text("V 等級上限"),
                );
                ui.checkbox(&mut self.options.lines, "星座線");
                ui.checkbox(&mut self.options.labels, "星座略号");
                if let Some(data) = &self.data {
                    ui.label(format!(
                        "{}\n恒星 {} 件・星座 {} 件",
                        data.root.display(),
                        data.stars.len(),
                        data.constellations.len()
                    ));
                }
            });
        self.settings = settings;
        egui::Window::new("選択天体の情報")
            .open(&mut self.info)
            .show(ctx, |ui| {
                if let Some((body, data)) = self
                    .solar
                    .layers
                    .selected(self.view)
                    .zip(self.data.as_ref())
                {
                    if let Ok(bodies) = planet_layers::positions(&data.ephemeris, self.jd) {
                        if let Some(b) = bodies.iter().find(|b| b.id == body) {
                            ui.label(format!(
                                "{}\n赤経 {:.5}°　赤緯 {:+.5}°\n地心距離 {:.8} AU",
                                solar::NAMES[body],
                                b.equatorial[0],
                                b.equatorial[1],
                                b.distance_au
                            ));
                            return;
                        }
                    }
                }
                if let Some((data, hip)) = self.data.as_ref().zip(self.selected) {
                    if let Some(&index) = data.by_hip.get(&hip) {
                        let star = &data.stars[index];
                        ui.label(format!(
                            "HIP {hip}\n赤経 {:.5}°　赤緯 {:+.5}°\nV 等級 {:.2}　B−V {:.2}",
                            star.ra_deg, star.dec_deg, star.magnitude, star.bv
                        ));
                    }
                } else {
                    ui.label("恒星をクリックするか、HIP 番号で検索してください。");
                }
            });
        if let Some(err) = self.error.clone() {
            let mut open = true;
            egui::Window::new("エラー").open(&mut open).show(ctx, |ui| {
                ui.label(&err);
                if ui.button("閉じる").clicked() {
                    self.error = None;
                }
            });
            if !open {
                self.error = None;
            }
        }
    }
    fn canvas(&mut self, ui: &mut egui::Ui) {
        if self.data.is_none() {
            ui.centered_and_justified(|ui| {
                if ui.button("天文データを開く…").clicked() {
                    self.open();
                }
            });
            return;
        }
        self.canvas_rect = ui.available_rect_before_wrap();
        if self.view == View::Solar {
            if ui.rect_contains_pointer(self.canvas_rect)
                && ui.input(|i| {
                    i.pointer.any_pressed() || i.smooth_scroll_delta.y != 0. || i.zoom_delta() != 1.
                })
            {
                self.interrupt_journey();
            }
            let data = self.data.as_ref().unwrap();
            let galaxies = self
                .galaxies
                .get_or_insert_with(|| galaxies::Browser::new(data.root.clone(), ui.ctx()));
            let view = solar::ViewSettings {
                data,
                jd: self.jd,
                picking: self.picking,
                terrain_mode: self.terrain_amplification,
                layers: &self.options.layers,
                galaxies,
                surface_site: self.motion.site.as_mut(),
                observer: [self.latitude, self.longitude],
                star_display: solar::sky::Display {
                    magnitude: self.options.magnitude,
                    lines: self.options.lines,
                    labels: self.options.labels,
                    selected: self.selected,
                },
            };
            if let Err(e) = self.solar.draw(ui, view) {
                ui.label(e.to_string());
            }
            if let Some(hit) = self.solar.sky_pick {
                self.selected = match hit {
                    solar::sky::Selection::Star(hip) => Some(hip),
                    solar::sky::Selection::Constellation(_) => None,
                };
            }
            if let Some(solar::sky::Selection::Constellation(i)) = self.solar.sky_selection {
                if let Some(r) = data.constellations.get(i) {
                    ui.painter().text(
                        ui.clip_rect().left_bottom() + egui::vec2(10., -8.),
                        egui::Align2::LEFT_BOTTOM,
                        format!(
                            "選択した星座　{}",
                            layers::catalog::decode(&r.japanese_kanji_bytes)
                        ),
                        egui::FontId::proportional(13.),
                        egui::Color32::GOLD,
                    );
                }
            }
            self.object_in_solar(ui);
            self.script_marker(ui);
            return;
        }
        if self.view == View::Surface {
            if let Some(surface) = &mut self.surface {
                if let Some(data) = &self.data {
                    surface.draw(ui, &data.ephemeris, self.jd);
                }
            }
            return;
        }
        let (response, painter) =
            ui.allocate_painter(ui.available_size(), egui::Sense::click_and_drag());
        let rect = response.rect;
        if rect.width() < 1. || rect.height() < 1. {
            return;
        }
        let view = self.view;
        let camera = self.camera();
        if response.dragged() {
            let delta = response.drag_delta();
            camera.pan(
                view,
                f64::from(delta.x / rect.width()),
                f64::from(delta.y / rect.height()),
            );
        }
        if let Some(mouse) = response.hover_pos() {
            let factor = ui
                .input(|i| (i.smooth_scroll_delta.y * 0.008).clamp(-2., 2.).exp() * i.zoom_delta());
            if (factor - 1.).abs() > 1e-6 {
                camera.zoom_at(
                    view,
                    factor.into(),
                    [
                        f64::from((mouse.x - rect.left()) / rect.width()),
                        f64::from((mouse.y - rect.top()) / rect.height()),
                    ],
                );
            }
        }
        let camera = *camera;
        let Ok(frame) = ObservationFrame::new(self.latitude, self.longitude, self.jd) else {
            return;
        };
        let chart = Chart {
            view,
            camera,
            frame: &frame,
            rect,
        };
        let pointer = self.picking.pointer(&response);
        let Some(data) = self.data.as_ref() else {
            return;
        };
        self.background_render.paint(
            &painter,
            &chart,
            &data.layers.backgrounds,
            self.options.layers.milky_way,
            [self.latitude, self.longitude, self.jd],
        );
        let show_landscape = view == View::Horizon && self.options.layers.landscape;
        if show_landscape {
            self.landscape_render.prepare(
                &painter,
                &data.layers.landscapes,
                &data.layers.places,
                &self.options.layers.observer,
                (camera, camera.roll),
                rect,
            );
        }
        let opaque = |p| show_landscape && self.landscape_render.opaque_at(p);
        let click = pointer.filter(|p| !opaque(*p));
        let pick = chart.paint(&painter, data, &self.options, self.selected, click);
        let bodies = planet_layers::positions(&data.ephemeris, self.jd).ok();
        let mut body_pick = bodies
            .as_ref()
            .and_then(|b| self.solar.layers.paint(&chart, &painter, b, click));
        let mut star_pick = pick;
        self.object_on_sky(&chart, &painter);
        let mut moving = false;
        if view == View::Horizon {
            // 候補の中心とクリック地点の両方を風景と照合し、近い実点を優先する。
            let distance = |q: [f64; 2]| {
                let xy = frame.to_horizontal(q[0], q[1]).ok()?;
                let p = chart.point(xy);
                if opaque(p) {
                    None
                } else {
                    Some(p.distance_sq(click?))
                }
            };
            let star_distance = pick.and_then(|hip| {
                let s = data.stars.get(*data.by_hip.get(&hip)?)?;
                distance([f64::from(s.ra_deg), f64::from(s.dec_deg)])
            });
            let body_distance = body_pick
                .and_then(|id| distance(bodies.as_ref()?.iter().find(|p| p.id == id)?.equatorial));
            if body_distance.is_none() {
                body_pick = None
            }
            if star_distance.is_none() {
                star_pick = None
            }
            if matches!((star_distance,body_distance),(Some(a),Some(b)) if a < b && a<=64.) {
                body_pick = None
            }
            let nearest = star_distance
                .unwrap_or(f32::INFINITY)
                .min(body_distance.unwrap_or(f32::INFINITY));
            moving = self.objects.paint_horizon(
                data,
                [self.jd, self.latitude, self.longitude],
                objects::horizon::Canvas {
                    chart: &chart,
                    painter: &painter,
                    pointer,
                    nearest,
                    opaque: &opaque,
                    picking: self.picking,
                },
            );
        }
        if show_landscape {
            self.landscape_render.paint(&painter, rect);
        }
        if pointer.is_some() {
            if moving {
                body_pick = None;
                star_pick = None;
            }
            self.solar.layers.select(view, body_pick);
            self.selected = if body_pick.is_some() { None } else { star_pick };
            ui.ctx().request_repaint();
        }
    }
}
impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        self.camera_frame(ctx);
        self.clock_frame(ctx);
        if self
            .options
            .layers
            .sample_brightness(std::time::Instant::now())
        {
            ctx.request_repaint_after(std::time::Duration::from_millis(16));
        }
        self.scripts_frame(ctx);
        self.media.frame(ctx);
        if self.view == View::Solar {
            if let Some(body) = self.solar.landed.take() {
                self.solar.stop();
                if body == 3 {
                    self.view = View::Horizon;
                } else if let Some(ground) = self.surface.as_ref().filter(|g| g.body == body) {
                    self.motion.site = Some(camera_control::session::Site::from_surface(
                        ground,
                        self.horizon,
                    ));
                    self.view = View::Solar;
                }
            }
        }
        if !ctx.wants_keyboard_input()
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::S))
        {
            self.execute(Command::Land, ctx);
        }
        for command in self.menus.poll(ctx) {
            self.execute(command, ctx);
        }
        if !ctx.wants_keyboard_input() {
            for (key, command) in [(Key::F, Command::Search), (Key::O, Command::Open)] {
                if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, key)) {
                    self.execute(command, ctx);
                }
            }
        }
        let canvas_layer = egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(Color32::from_rgb(6, 11, 18)))
            .show(ctx, |ui| {
                self.canvas(ui);
                ui.layer_id()
            })
            .inner;
        self.quality.apply(ctx, canvas_layer, &self.options.layers);
        self.projector.frame(ctx, canvas_layer, self.canvas_rect);
        if self.playback.show && self.data.is_some() {
            egui::Area::new("current date".into())
                .anchor(egui::Align2::LEFT_BOTTOM, [8., -8.])
                .interactable(false)
                .show(ctx, |ui| {
                    if let Some(date) = utc_date(self.jd) {
                        ui.label(
                            egui::RichText::new(date.format("%Y-%m-%d %H:%M:%S UTC").to_string())
                                .background_color(Color32::BLACK)
                                .color(Color32::WHITE),
                        );
                    }
                });
        }
        self.capture_frame(ctx);
        if self.capture.is_none() {
            self.panels(ctx);
        }
        if let Some(mut smoke) = self.smoke.take() {
            if !smoke.frame(self, ctx) {
                self.smoke = Some(smoke);
            }
        }
    }
}
fn run() -> Result<(), AppError> {
    let args = Args::parse(env::args_os().skip(1))?;
    if args.help {
        println!("mitaka-plus [--data-root DIR] [--inspect | --smoke-test]");
        return Ok(());
    }
    let root = args
        .data
        .or_else(|| env::var_os("DATA_ROOT").map(PathBuf::from))
        .or_else(|| {
            let path = PathBuf::from("resources/data");
            path.join("hip.dat").is_file().then_some(path)
        });
    let (data, error) = match root {
        Some(path) => match SkyData::open(&path) {
            Ok(data) => (Some(data), None),
            Err(error) => (None, Some(format!("{}: {error}", path.display()))),
        },
        None => (None, None),
    };
    if args.inspect || args.smoke {
        let data = data.as_ref().ok_or_else(|| {
            error
                .clone()
                .unwrap_or_else(|| "--data-root にデータの場所を指定してください".into())
        })?;
        let state = data.ephemeris.state(0, J2000)?;
        println!(
            "stars={} constellations={} mercury_j2000_au={:?}",
            data.stars.len(),
            data.constellations.len(),
            state.position_au
        );
        if args.inspect {
            return Ok(());
        }
    }
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1080., 700.])
            .with_min_inner_size([400., 300.]),
        ..Default::default()
    };
    let (smoke, rx) = if args.smoke {
        let (s, r) = smoke::Smoke::new();
        (Some(s), Some(r))
    } else {
        (None, None)
    };
    eframe::run_native(
        "Mitaka Plus",
        options,
        Box::new(move |cc| Ok(Box::new(App::new(cc, data, error, smoke)?))),
    )
    .map_err(|e| e.to_string())?;
    if let Some(rx) = rx {
        rx.try_recv()
            .map_err(|_| "描画検証中にウィンドウが閉じられました")??;
    }
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn julian_epoch_and_utc_date() {
        assert_eq!(parse_utc("2000-01-01 12:00:00"), Some(J2000));
        assert_eq!(
            utc_date(UNIX_EPOCH_JD).unwrap().to_rfc3339(),
            "1970-01-01T00:00:00+00:00"
        );
        assert_eq!(parse_utc("2001-02-29 12:00:00"), None);
    }
    #[test]
    fn reject_missing_path_or_conflicting_modes() {
        for args in [
            vec!["--data-root"],
            vec!["--data-root", "--inspect"],
            vec!["--inspect", "--smoke-test"],
        ] {
            assert!(Args::parse(args.into_iter().map(OsString::from)).is_err());
        }
    }
}
