mod camera_control;
mod capture;
mod catalog;
mod chart;
mod chart_input;
mod commands;
mod data_loading;
mod deep_sky;
mod fonts;
mod fullscreen;
mod galaxies;
mod layers;
mod media_preview;
mod objects;
mod planet_layers;
mod planet_scale;
mod playback;
mod preferences;
mod projection;
mod projector;
mod script_ui;
mod session;
mod session_ui;
mod settings_ui;
mod shortcuts;
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
    // 秒表示の丸めは同じ暦日内に収める。月末の23:59:59.75を翌月と表示しない。
    let days = jd - UNIX_EPOCH_JD;
    let midnight = days.floor();
    let seconds = ((days - midnight) * 86400.).round().min(86399.);
    DateTime::from_timestamp((midnight * 86400. + seconds) as i64, 0)
}
fn parse_utc(text: &str) -> Option<f64> {
    let date = NaiveDateTime::parse_from_str(text.trim(), "%Y-%m-%d %H:%M:%S").ok()?;
    Some(UNIX_EPOCH_JD + date.and_utc().timestamp() as f64 / 86400.)
}

fn update_observation_date(
    jd: &mut f64,
    utc: &mut String,
    requested: f64,
    date_range: Option<[f64; 2]>,
) -> Result<(), &'static str> {
    mitaka_core::astronomy::gregorian_date_time(requested)
        .map_err(|_| "日時が天文計算の有効範囲外です")?;
    if date_range.is_some_and(|[start, end]| !(start..=end).contains(&requested)) {
        return Err("日時がCF96暦表の有効範囲外です");
    }
    let date = utc_date(requested).ok_or("日時の範囲外です")?;
    *utc = date.format("%Y-%m-%d %H:%M:%S").to_string();
    *jd = requested;
    Ok(())
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
    fullscreen: fullscreen::Fullscreen,
    media: media_preview::Media,
    scripts: script_ui::Scripts,
    playback: playback::Playback,
    session: session::Session,
    data: Option<SkyData>,
    pending_data: Option<data_loading::Request>,
    preferences: preferences::Preferences,
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
    time_settings: bool,
    place_settings: bool,
    info: bool,
    about: bool,
    search_message: String,
    error: Option<String>,
    utc: String,
    jd_input: String,
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
        let jd = initial
            .as_ref()
            .map_or(J2000, |data| data.initial_date(J2000));
        let utc = utc_date(jd).ok_or("日時の範囲外です")?;
        let mut media = media_preview::Media::default();
        if let Some(data) = &initial {
            media.reset(&data.root);
        }
        let (preferences, error) = if smoke.is_some() {
            (preferences::Preferences::Disabled, error)
        } else {
            match preferences::config_dir() {
                Ok(dir) => (
                    preferences::Preferences::Unread(dir.join("display-v9.json")),
                    error,
                ),
                Err(e) => (preferences::Preferences::Disabled, Some(e.to_string())),
            }
        };
        let mut app = Self {
            picking: Default::default(),
            quality: Default::default(),
            terrain_amplification: Default::default(),
            data: initial,
            pending_data: None,
            preferences,
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
            fullscreen: fullscreen::Fullscreen::new(cc)?,
            menus,
            view: View::Sky,
            sky: Camera::default(),
            horizon: Camera::default(),
            options: Options::default(),
            jd,
            latitude: 35.68,
            longitude: 139.76,
            selected: None,
            query: String::new(),
            search: false,
            settings: false,
            time_settings: false,
            place_settings: false,
            info: false,
            about: false,
            search_message: String::new(),
            error,
            jd_input: format!("{jd:.6}"),
            utc: utc.format("%Y-%m-%d %H:%M:%S").to_string(),
            smoke,
        };
        app.restore_preferences();
        Ok(app)
    }
    fn set_terrain_mode(
        &mut self,
        mode: mitaka_core::terrain::Amplification,
    ) -> std::io::Result<()> {
        if mode == self.terrain_amplification {
            return Ok(());
        }
        let current = self
            .motion
            .site
            .or(self.solar.flight_site)
            .filter(|s| self.view == View::Solar && matches!(s.body, 3 | 4));
        if let Some(s) = current.filter(|s| {
            self.surface
                .as_ref()
                .is_none_or(|g| !s.at_location(g.body, g.lat, g.lon))
        }) {
            if let Some(data) = &self.data {
                self.surface = Some(surface::Surface::load_mode(
                    &data.root, s.body, s.lat, s.lon, 100., mode,
                )?);
            }
        }
        if let Some(surface) = &mut self.surface {
            surface.set_mode(mode)?;
            if let Some(s) = self
                .motion
                .site
                .as_mut()
                .filter(|s| s.at_location(surface.body, surface.lat, surface.lon))
            {
                s.update_terrain(surface.landing_height_m(solar::RADII_KM[s.body]) / 1000.);
            }
            self.solar.update_ground(surface, self.jd)?;
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
    fn execute(&mut self, command: Command, ctx: &egui::Context) {
        if self.pending_data.is_some() && !matches!(command, Command::Open | Command::Quit) {
            return;
        }
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
            Command::Quit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            Command::ZoomIn | Command::ZoomOut => {
                let view = self.view;
                if matches!(view, View::Sky | View::Horizon) {
                    let factor = if command == Command::ZoomIn {
                        1.5
                    } else {
                        1. / 1.5
                    };
                    self.zoom_chart_at(factor, [0.5, 0.5]);
                }
            }
            Command::Open => self.open(ctx),
            Command::Media => self.media.choose(ctx),
            Command::MediaList => self.media.browser = true,
            Command::Projector => {
                self.projector.controls = true;
                self.projector.open = true;
            }
            Command::Fullscreen => {
                if let Err(error) = self.fullscreen.request(ctx, None) {
                    self.error = Some(error.into());
                }
            }
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
            Command::TimeSettings => self.time_settings = true,
            Command::PlaceSettings => self.place_settings = true,
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
                    if let Err(e) = self.nonterrain_mode(ascend, self.motion.mode_duration, false) {
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
                    let site = camera_control::session::Site::ground(
                        body,
                        lat,
                        lon,
                        ground.landing_height_m(solar::RADII_KM[body]),
                        self.horizon,
                    );
                    self.solar.begin_surface_flight(site, self.jd, ascend)?;
                    self.solar.flight_duration(self.motion.mode_duration);
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
            Command::Reset if self.view == View::Solar => {
                self.motion.reset_solar_camera(&mut self.solar);
                self.objects.stop_tracking();
            }
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
        if self.solar.panel(ctx) {
            if let Some(data) = &self.data {
                match self
                    .solar
                    .prepare(&data.ephemeris, self.jd)
                    .and_then(|_| self.solar.body_centers())
                {
                    Ok(centers) => {
                        let (earth, moon) = (centers[3], centers[10]);
                        self.motion.reset_solar_camera(&mut self.solar);
                        self.objects.stop_tracking();
                        self.solar.show_earth_moon(earth, moon);
                        self.view = View::Solar;
                    }
                    Err(e) => self.error = Some(e.to_string()),
                }
            }
        }
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
        self.settings_windows(ctx);
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
                    self.open(ui.ctx());
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
                horizon_camera: self.horizon,
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
            self.draw_horizon_flight(ui);
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
        if response.dragged() {
            let delta = response.drag_delta();
            self.camera().pan(
                view,
                f64::from(delta.x / rect.width()),
                f64::from(delta.y / rect.height()),
            );
        }
        if let Some(mouse) = response.hover_pos() {
            let (pan, factor) = ui.input(chart_input::gesture);
            if pan != egui::Vec2::ZERO {
                self.camera().pan(
                    view,
                    f64::from(pan.x / rect.width()),
                    f64::from(pan.y / rect.height()),
                );
            }
            if (factor - 1.).abs() > 1e-6 {
                self.zoom_chart_at(
                    factor.into(),
                    [
                        f64::from((mouse.x - rect.left()) / rect.width()),
                        f64::from((mouse.y - rect.top()) / rect.height()),
                    ],
                );
            }
        }
        let camera = *self.camera();
        let Ok(frame) = ObservationFrame::new(self.latitude, self.longitude, self.jd) else {
            return;
        };
        let chart = Chart {
            flight: None,
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
        let mut pick = pointer
            .filter(|p| !opaque(*p))
            .map(|p| chart::ChartPick::new(p, &opaque));
        chart.paint(&painter, data, &self.options, self.selected, pick.as_mut());
        if let Ok(bodies) = planet_layers::positions(&data.ephemeris, self.jd) {
            self.solar
                .layers
                .paint(&chart, &painter, &bodies, pick.as_mut());
        }
        self.object_on_sky(&chart, &painter);
        let moving = if view == View::Horizon {
            self.objects.paint_horizon(
                data,
                [self.jd, self.latitude, self.longitude],
                objects::horizon::Canvas {
                    chart: &chart,
                    painter: &painter,
                    pointer,
                    nearest: pick.as_ref().map_or(f32::INFINITY, |p| p.distance_sq),
                    opaque: &opaque,
                    picking: self.picking,
                },
            )
        } else {
            None
        };
        if show_landscape {
            self.landscape_render.paint(&painter, rect);
        }
        if pointer.is_some() {
            let hit = pick.and_then(|p| p.target).filter(|_| moving.is_none());
            let focused = moving.or_else(|| hit.and_then(|(_, [ra, dec])| chart.observed(ra, dec)));
            let (star, body) = match hit.map(|t| t.0) {
                Some(chart::ChartTarget::Star(hip)) => (Some(hip), None),
                Some(chart::ChartTarget::Body(id)) => (None, Some(id)),
                None => (None, None),
            };
            self.solar.layers.select(view, body);
            self.selected = star;
            chart_input::focus(
                self.picking,
                response.double_clicked(),
                focused,
                view,
                self.camera(),
            );
            ui.ctx().request_repaint();
        }
    }
    fn draw_horizon_flight(&mut self, ui: &egui::Ui) {
        let Some(f) = self.solar.horizon_overlay else {
            return;
        };
        let Some(site) = self.solar.flight_site else {
            return;
        };
        let Some(data) = &self.data else { return };
        let Ok(frame) = ObservationFrame::new(site.lat, site.lon, self.jd) else {
            return;
        };
        let chart = Chart {
            view: View::Horizon,
            rect: f.rect,
            camera: f.camera,
            frame: &frame,
            flight: Some(f),
        };
        let mut painter = ui.painter().with_clip_rect(f.rect);
        painter.multiply_opacity((1. - f.blend) as f32);
        painter.rect_filled(f.rect, 0., Color32::from_rgb(6, 11, 18));
        self.background_render.paint(
            &painter,
            &chart,
            &data.layers.backgrounds,
            self.options.layers.milky_way,
            [site.lat, site.lon, self.jd],
        );
        chart.paint(&painter, data, &self.options, self.selected, None);
        if let Ok(bodies) = planet_layers::positions(&data.ephemeris, self.jd) {
            self.solar.layers.paint(&chart, &painter, &bodies, None);
        }
        self.object_on_sky(&chart, &painter);
        if self.options.layers.landscape {
            self.landscape_render.prepare(
                &painter,
                &data.layers.landscapes,
                &data.layers.places,
                &self.options.layers.observer,
                (f.camera, f.camera.roll),
                f.rect,
            );
            self.landscape_render.paint_flight(&painter, f);
        }
    }
}
impl eframe::App for App {
    fn on_exit(&mut self) {
        if let Err(error) = self.save_preferences() {
            eprintln!("表示設定を保存できません: {error}");
        }
    }

    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        if let Err(error) = self.fullscreen.poll(ctx) {
            self.error = Some(error.into());
        }
        self.poll_data();
        for command in self.menus.poll(ctx) {
            self.execute(command, ctx);
        }
        if !ctx.wants_keyboard_input() {
            let chart = matches!(self.view, View::Sky | View::Horizon);
            for command in shortcuts::commands(ctx, chart) {
                self.execute(command, ctx);
            }
        }
        if self.pending_data.is_some() {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.spinner();
                ui.label("天文データを読み込んでいます");
                if ui.button("別のデータを開く…").clicked() {
                    self.open(ctx);
                }
            });
            return;
        }
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
                let arrival = self.solar.flight_site;
                self.solar.stop();
                if let Some(site) = arrival {
                    self.horizon.longitude = site.heading;
                    self.horizon.latitude = site.elevation;
                    self.horizon.roll = site.roll;
                    if body == 3 {
                        self.latitude = site.lat;
                        self.longitude = site.lon;
                    }
                }
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
    let (inspection_root, startup_root) = if args.inspect || args.smoke {
        (root, None)
    } else {
        (None, root)
    };
    let (data, error) = match inspection_root {
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
        let jd = data.initial_date(J2000);
        let state = data.ephemeris.state(0, jd)?;
        let coordinate = if jd == J2000 {
            "mercury_j2000_au".into()
        } else {
            format!("jd={jd} mercury_au")
        };
        println!(
            "stars={} constellations={} {coordinate}={:?}",
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
        Box::new(move |cc| {
            let mut app = App::new(cc, data, error, smoke)?;
            if let Some(root) = startup_root {
                app.load_data(root, &cc.egui_ctx);
            }
            Ok(Box::new(app))
        }),
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
    fn next_day_preserves_the_last_renderable_date() {
        let mut jd = J2000;
        let mut utc = String::new();
        let last = parse_utc("9999-12-31 23:59:59").unwrap();
        update_observation_date(&mut jd, &mut utc, last, None).unwrap();
        assert!(utc_date(last + 1.).is_some());
        assert_eq!(
            update_observation_date(&mut jd, &mut utc, last + 1., None),
            Err("日時が天文計算の有効範囲外です")
        );
        assert_eq!(jd, last);
        assert_eq!(utc, "9999-12-31 23:59:59");
        assert!(ObservationFrame::new(35., 139., jd).is_ok());
    }
    #[test]
    fn date_changes_respect_both_ends_of_the_loaded_ephemeris() {
        let (mut jd, mut utc) = (J2000, String::new());
        let range = Some([J2000, J2000 + 1.]);
        for (endpoint, outside) in [(J2000, J2000 - 1.), (J2000 + 1., J2000 + 2.)] {
            update_observation_date(&mut jd, &mut utc, endpoint, range).unwrap();
            let previous = utc.clone();
            assert_eq!(
                update_observation_date(&mut jd, &mut utc, outside, range),
                Err("日時がCF96暦表の有効範囲外です")
            );
            assert_eq!(jd, endpoint);
            assert_eq!(utc, previous);
        }
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
