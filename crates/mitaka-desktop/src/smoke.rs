//! 実ウィンドウからGPU描画結果を読み戻して、各シーンの切替と表示を検査する。
use super::{capture, galaxies, projection::View, surface, App};
use eframe::egui;
use std::{
    collections::HashSet,
    env, io,
    path::PathBuf,
    sync::mpsc,
    time::{Duration, Instant},
};

fn check_date_limits(app: &mut App) -> io::Result<()> {
    fn reject_next_to(app: &mut App, valid: f64, invalid: f64) -> io::Result<()> {
        app.apply_date(valid).map_err(io::Error::other)?;
        let utc = app.utc.clone();
        app.set_date(invalid);
        if app.error.take().is_none() || app.jd != valid || app.utc != utc {
            return Err(io::Error::other(format!(
                "範囲外の日時 {invalid} を指定した後に {valid} と {utc} を保持できません"
            )));
        }
        Ok(())
    }

    let initial = app.jd;
    let last = super::parse_utc("9999-12-31 23:59:59")
        .ok_or_else(|| io::Error::other("暦の上限を変換できません"))?;
    let data = app.data.take();
    let original_error = app.error.take();
    let calendar = reject_next_to(app, last, last + 1.);
    app.data = data;
    calendar?;
    let [first, last] = app
        .data
        .as_ref()
        .ok_or_else(|| io::Error::other("暦表が必要です"))?
        .date_range;
    reject_next_to(app, first, first - 1.)?;
    reject_next_to(app, last, last + 1.)?;
    app.jd_input = first.to_string();
    app.submit_julian_input();
    if app.jd != first || app.jd_input != format!("{first:.6}") {
        return Err(io::Error::other("JD欄で暦表の初日を適用できません"));
    }
    let utc = app.utc.clone();
    for input in [
        "invalid".into(),
        "NaN".into(),
        "inf".into(),
        (last + 1.).to_string(),
    ] {
        app.jd_input = input;
        app.submit_julian_input();
        if app.error.take().is_none()
            || app.jd != first
            || app.utc != utc
            || app.jd_input != format!("{first:.6}")
        {
            return Err(io::Error::other("JD欄で無効な入力から元の日時に戻れません"));
        }
    }
    app.error = original_error;
    app.apply_date(initial).map_err(io::Error::other)
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Scene {
    Sky,
    DeepSky,
    Horizon,
    Solar,
    ReferenceSurface,
    GlobeEarth,
    GlobeMars,
    Earth,
    Mars,
    Galaxy,
    Sdss,
    SolarReturn,
    ObjectBrowser,
    SpacecraftPaths,
    CameraLua,
}
impl Scene {
    fn name(self) -> &'static str {
        match self {
            Self::Sky => "sky",
            Self::DeepSky => "deep-sky",
            Self::Horizon => "horizon",
            Self::Solar => "solar",
            Self::ReferenceSurface => "jupiter-surface",
            Self::GlobeEarth => "earth-sphere",
            Self::GlobeMars => "mars-sphere",
            Self::Earth => "earth",
            Self::Mars => "mars",
            Self::Galaxy => "galaxy",
            Self::Sdss => "sdss-3d",
            Self::SolarReturn => "solar-return",
            Self::ObjectBrowser => "object-browser",
            Self::SpacecraftPaths => "spacecraft-paths",
            Self::CameraLua => "camera-lua",
        }
    }
}
pub struct Smoke {
    scenes: Vec<Scene>,
    index: usize,
    entered: bool,
    browser_hip: Option<i32>,
    waiting: bool,
    start: Instant,
    result: mpsc::Sender<Result<(), String>>,
    captures: Option<PathBuf>,
}
impl Smoke {
    pub fn new() -> (Self, mpsc::Receiver<Result<(), String>>) {
        let (tx, rx) = mpsc::channel();
        (
            Self {
                scenes: vec![
                    Scene::Sky,
                    Scene::DeepSky,
                    Scene::Horizon,
                    Scene::Solar,
                    Scene::ReferenceSurface,
                    Scene::GlobeEarth,
                    Scene::GlobeMars,
                    Scene::Earth,
                    Scene::Mars,
                    Scene::Galaxy,
                    Scene::Sdss,
                    Scene::SolarReturn,
                    Scene::ObjectBrowser,
                    Scene::SpacecraftPaths,
                    Scene::CameraLua,
                ],
                index: 0,
                entered: false,
                browser_hip: None,
                waiting: false,
                start: Instant::now(),
                result: tx,
                captures: env::var_os("MITAKA_SMOKE_CAPTURE_DIR").map(PathBuf::from),
            },
            rx,
        )
    }
    fn enter(&mut self, app: &mut App, scene: Scene, ctx: &egui::Context) -> io::Result<bool> {
        let data = app
            .data
            .as_ref()
            .ok_or_else(|| io::Error::other("データをロードできません"))?;
        match scene {
            Scene::Sky => {
                super::data_loading::check_switch(app, ctx)?;
                super::preferences::check_restart(app)?;
                check_date_limits(app)?;
                app.view = View::Sky;
                app.projector.open = true;
                app.projector.probe = true;
            }
            Scene::DeepSky => {
                if !["ngc.dat", "SDSS_GAL.dat", "SDSS_QSO.dat"]
                    .iter()
                    .all(|f| data.root.join(f).is_file())
                {
                    return Ok(false);
                }
                app.projector.open = false;
                app.view = View::Sky;
                let d = &mut app.options.layers.deep_sky;
                d.ngc = true;
                d.galaxies = true;
                d.quasars = true;
            }
            Scene::Horizon => {
                app.options.layers.deep_sky = Default::default();
                app.view = View::Horizon;
                app.projector.open = false;
            }
            Scene::Solar => {
                // 単なるエラー文を描画成功として数えない。
                app.solar.prepare(&data.ephemeris, app.jd)?;
                app.view = View::Solar;
            }
            Scene::ReferenceSurface => {
                app.view = View::Solar;
                app.solar.prepare(&data.ephemeris, app.jd)?;
                // 基準球でも火星地表からの距離操作と、離陸中の許可条件を検査できる。
                app.solar.approach(4)?;
                app.nonterrain_mode(false, 0., true)?;
                let msg = serde_json::json!({"duration":0., "enabled":false, "number":1.});
                let distance = app.solar.camera.distance;
                for kind in [4, 103] {
                    if app.camera_action(kind, &msg, ctx).is_ok()
                        || app.solar.camera.distance != distance
                    {
                        return Err(io::Error::other("火星の地表でカメラ距離が変化しました"));
                    }
                }
                app.nonterrain_mode(true, 3., false)?;
                app.camera_action(103, &msg, ctx)
                    .map_err(io::Error::other)?;
                app.motion = Default::default();
                app.solar.approach(5)?;
                app.nonterrain_mode(false, 0., true)?;
                let ctx_value = serde_json::json!({"duration":0.,"enabled":true,"number":0.});
                app.camera_action(93, &ctx_value, ctx)
                    .map_err(io::Error::other)?;
            }
            Scene::GlobeEarth | Scene::GlobeMars => {
                app.motion = Default::default();
                let texture = if scene == Scene::GlobeEarth {
                    "earth.jpg"
                } else {
                    "mars.jpg"
                };
                if !data
                    .root
                    .with_file_name("textures")
                    .join(texture)
                    .try_exists()?
                {
                    eprintln!("SKIP {} ({texture} がありません)", scene.name());
                    return Ok(false);
                }
                let body = if scene == Scene::GlobeEarth { 3 } else { 4 };
                app.solar.prepare(&data.ephemeris, app.jd)?;
                app.solar.approach(body)?;
                app.view = View::Solar;
            }
            Scene::Earth | Scene::Mars => {
                let (index, stem) = if scene == Scene::Mars {
                    (4, "mars")
                } else {
                    (3, "earth")
                };
                let path = data.root.join(format!("{stem}_topo/{stem}_topo.dat"));
                if !path.try_exists()? {
                    eprintln!("SKIP {}（{} がありません）", scene.name(), path.display());
                    return Ok(false);
                }
                let ground =
                    surface::Surface::load(&data.root, index, app.latitude, app.longitude, 100.)?;
                println!("{} height_m={:.2}", scene.name(), ground.central_height);
                app.surface = Some(ground);
                app.view = View::Surface;
            }
            Scene::CameraLua => {
                app.start_camera_check(ctx)?;
            }
            Scene::SpacecraftPaths => {
                if !data
                    .root
                    .parent()
                    .is_some_and(|p| p.join("spacecraft/Voyager_I.pth").is_file())
                {
                    return Ok(false);
                }
                app.objects.inspect_voyager(data)?;
                app.view = View::Solar;
            }
            Scene::ObjectBrowser => {
                self.browser_hip = Some(app.objects.inspect_star(data)?);
                app.view = View::Sky;
            }
            Scene::Galaxy => {
                if !data.root.join("ourgalaxy_model.dat").try_exists()? {
                    eprintln!("SKIP galaxy（天の川モデルがありません）");
                    return Ok(false);
                }
                app.galaxies = Some(galaxies::Browser::new(data.root.clone(), ctx));
                app.execute(crate::commands::Command::Galaxy, ctx);
                app.galaxies.as_mut().unwrap().controls = false;
            }
            Scene::Sdss => {
                if !data.root.join("SDSS_GAL.dat").is_file() {
                    return Ok(false);
                }
                let browser = app
                    .galaxies
                    .get_or_insert_with(|| galaxies::Browser::new(data.root.clone(), ctx));
                browser.select_sdss(&mut app.options.layers.deep_sky);
                app.solar.camera.set_target_pc([0.; 3]);
                app.solar.camera.distance = 1.8e9 * crate::solar::precise::AU_PER_PC;
                app.view = View::Solar;
            }
            Scene::SolarReturn => {
                let dir = tempfile::tempdir()?;
                let path = dir.path().join("galaxy-v9.json");
                app.solar.camera.pan_au([0.0003, -0.0002, 0.0001]);
                let before = app.solar.camera;
                app.save_session(&path)?;
                app.restore_session(&path)?;
                if app.view != View::Solar || app.solar.camera != before {
                    return Err(io::Error::other(format!(
                        "銀河視点のv9往復でカメラが変わりました {before:?} → {:?}",
                        app.solar.camera
                    )));
                }
                app.execute(crate::commands::Command::Reset, ctx);
                app.options.layers.deep_sky = Default::default();
                if app.solar.camera.target != [0.; 3] || app.solar.camera.precise().is_some() {
                    return Err(io::Error::other("太陽系の初期視点に戻れません"));
                }
            }
        }
        Ok(true)
    }
    fn advance(&mut self, app: &mut App, ctx: &egui::Context) -> io::Result<bool> {
        if self.start.elapsed() > Duration::from_secs(90) {
            return Err(io::Error::other(
                "GPUからの描画結果取得がタイムアウトしました",
            ));
        }
        let Some(&scene) = self.scenes.get(self.index) else {
            return Ok(true);
        };
        if !self.entered {
            if self.enter(app, scene, ctx)? {
                self.entered = true;
            } else {
                self.index += 1;
            }
            return Ok(false);
        }
        if scene == Scene::Sky && app.projector.open {
            if app.projector.verified() {
                app.projector.open = false;
            }
            return Ok(false);
        }
        if scene == Scene::DeepSky && !app.data.as_ref().unwrap().deep_sky.borrow().ready()? {
            return Ok(false);
        }
        if scene == Scene::CameraLua {
            if !app.camera_check_complete()? {
                return Ok(false);
            }
            if !self.waiting {
                app.check_camera_session()?;
            }
        }
        if matches!(scene, Scene::Galaxy | Scene::Sdss) {
            let Some(g) = &app.galaxies else {
                return Ok(false);
            };
            if !g.ready()? {
                return Ok(false);
            }
        }
        if scene == Scene::ObjectBrowser {
            let data = app.data.as_ref().unwrap();
            let hip = self.browser_hip.unwrap();
            let star = &data.stars[data.by_hip[&hip]];
            if app.selected != Some(hip)
                || app.sky.longitude != f64::from(star.ra_deg)
                || app.sky.latitude != f64::from(star.dec_deg)
            {
                return Err(io::Error::other(format!(
                    "天体ブラウザからHIP {hip} へ視点が移りません"
                )));
            }
        }
        if scene == Scene::SpacecraftPaths && app.view != View::Solar {
            return Err(io::Error::other("探査機位置へ視点が移りません"));
        }
        if !self.waiting {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::new(
                scene,
            )));
            self.waiting = true;
            return Ok(false);
        }
        let image = ctx.input(|i| {
            i.events.iter().find_map(|event| {
                if let egui::Event::Screenshot {
                    user_data, image, ..
                } = event
                {
                    if user_data.data.as_ref()?.downcast_ref::<Scene>() == Some(&scene) {
                        return Some(image.clone());
                    }
                }
                None
            })
        });
        if let Some(image) = image {
            let unique: HashSet<_> = image
                .pixels
                .iter()
                .map(|p| p.to_array())
                .take(4_000_000)
                .collect();
            if image.width() < 100 || image.height() < 100 || unique.len() < 6 {
                return Err(io::Error::other(format!(
                    "{}: 表示画素が不足しています",
                    scene.name()
                )));
            }
            println!(
                "{} pixels={}x{} colors={}",
                scene.name(),
                image.width(),
                image.height(),
                unique.len()
            );
            if let Some(dir) = &self.captures {
                std::fs::create_dir_all(dir)?;
                let root = &app.data.as_ref().unwrap().root;
                capture::save(
                    &image,
                    &dir.join(format!("{}.png", scene.name())),
                    root,
                    app.canvas_rect,
                    ctx.pixels_per_point(),
                )?;
            }
            self.index += 1;
            self.entered = false;
            self.waiting = false;
        }
        Ok(false)
    }
    pub fn frame(&mut self, app: &mut App, ctx: &egui::Context) -> bool {
        match self.advance(app, ctx) {
            Ok(false) => {
                ctx.request_repaint_after(Duration::from_millis(15));
                false
            }
            result => {
                let _ = self
                    .result
                    .send(result.map(|_| ()).map_err(|e| e.to_string()));
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                true
            }
        }
    }
}
