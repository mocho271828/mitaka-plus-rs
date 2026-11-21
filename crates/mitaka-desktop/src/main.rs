mod capture;
mod catalog;
mod chart;
mod commands;
mod fonts;
mod galaxies;
mod media_preview;
mod objects;
mod playback;
mod projection;
mod script_ui;
mod session;
mod session_ui;
mod smoke;
mod solar;
mod surface;

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
        let jd = initial
            .as_ref()
            .map_or(J2000, |data| data.initial_date(J2000));
        let utc = utc_date(jd).ok_or("日時の範囲外です")?;
        Ok(Self {
            data: initial,
            media: Default::default(),
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
            jd,
            latitude: 35.68,
            longitude: 139.76,
            selected: None,
            query: String::new(),
            search: false,
            settings: false,
            info: false,
            search_message: String::new(),
            error,
            utc: utc.format("%Y-%m-%d %H:%M:%S").to_string(),
            smoke,
        })
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
                    let jd = data.initial_date(self.jd);
                    self.data = Some(data);
                    if jd != self.jd {
                        self.set_date(jd);
                    }
                    self.objects = Default::default();
                    self.solar = Default::default();
                    self.surface = None;
                    self.galaxies = None;
                    if matches!(self.view, View::Surface | View::Galaxy) {
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
        match command {
            Command::Open => self.open(),
            Command::Media => self.media.choose(ctx),
            Command::RunScript => self.script_file(ctx),
            Command::ScriptPanel => self.scripts.panel = true,
            Command::LocalHtml => self.open_script_html(),
            Command::LoadState => self.session_dialog(false),
            Command::SaveState => self.session_dialog(true),
            Command::Sky => self.view = View::Sky,
            Command::Horizon => self.view = View::Horizon,
            Command::Objects => self.objects.open = true,
            Command::Settings => self.settings = true,
            Command::Search => self.search = true,
            Command::Galaxy => {
                self.view = View::Galaxy;
                if let Some(g) = &mut self.galaxies {
                    g.controls = true;
                }
            }
            Command::Land => {
                let Some(data) = self.data.as_ref() else {
                    return;
                };
                let ascend = matches!(self.view, View::Horizon | View::Surface);
                let body = match self.view {
                    View::Solar => self.solar.selected,
                    View::Horizon => 3,
                    View::Surface => self.surface.as_ref().map_or(3, |s| s.body),
                    _ => {
                        self.error =
                            Some("地平図または太陽系で地球・火星を選択してください".into());
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
                let result = (|| -> std::io::Result<()> {
                    if body != 3 && body != 4 {
                        return Err(std::io::Error::other(
                            "地形の離着陸は地球と火星で利用できます",
                        ));
                    }
                    let ground = surface::Surface::load(&data.root, body, lat, lon, 100.)?;
                    self.solar.prepare(&data.ephemeris, self.jd)?;
                    self.solar.begin_surface_flight(
                        body,
                        lat,
                        lon,
                        ground.central_height,
                        self.jd,
                        ascend,
                    )?;
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
                match surface::Surface::load(&data.root, body, self.latitude, self.longitude, 100.)
                {
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
            Command::Reset if self.view == View::Galaxy => self.galaxies = None,
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
        if matches!(self.view, View::Solar | View::Surface | View::Galaxy) {
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
                if let egui::Event::Screenshot { image, .. } = e {
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
        self.object_panels(ctx);
        self.solar.panel(ctx);
        if let Some(g) = &mut self.galaxies {
            if self.view == View::Galaxy {
                g.panel(ctx);
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
                ui.add(egui::Slider::new(&mut self.latitude, -90. ..=90.).text("緯度 °"));
                ui.add(egui::Slider::new(&mut self.longitude, -180. ..=180.).text("東経 °"));
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
        if self.view == View::Galaxy {
            let Some(data) = self.data.as_ref() else {
                return;
            };
            let g = self.galaxies.get_or_insert_with(|| {
                let mut g = galaxies::Browser::new(data.root.clone(), ui.ctx());
                g.controls = true;
                g
            });
            g.draw(ui);
            return;
        }
        if self.view == View::Solar {
            let data = self.data.as_ref().unwrap();
            if let Err(e) = self.solar.draw(ui, &data.ephemeris, self.jd, &data.root) {
                ui.label(e.to_string());
            }
            self.object_in_solar(ui);
            return;
        }
        if self.view == View::Surface {
            if let Some(surface) = &mut self.surface {
                surface.draw(ui);
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
        let click = response
            .clicked()
            .then(|| response.interact_pointer_pos())
            .flatten();
        let Some(data) = self.data.as_ref() else {
            return;
        };
        let pick = chart.paint(&painter, data, &self.options, self.selected, click);
        self.object_on_sky(&chart, &painter);
        if click.is_some() {
            self.selected = pick;
            ui.ctx().request_repaint();
        }
    }
}
impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        self.clock_frame(ctx);
        self.scripts_frame(ctx);
        self.media.frame(ctx);
        if self.view == View::Solar {
            if let Some(body) = self.solar.landed.take() {
                self.view = if body == 3 {
                    View::Horizon
                } else {
                    View::Surface
                };
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
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(Color32::from_rgb(6, 11, 18)))
            .show(ctx, |ui| self.canvas(ui));
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
