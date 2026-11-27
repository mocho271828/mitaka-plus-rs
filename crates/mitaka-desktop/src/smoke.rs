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

#[derive(Clone, Copy, PartialEq, Debug)]
enum Scene {
    Sky,
    Horizon,
    Solar,
    GlobeEarth,
    GlobeMars,
    Earth,
    Mars,
    Galaxy,
    ObjectBrowser,
    SpacecraftPaths,
}
impl Scene {
    fn name(self) -> &'static str {
        match self {
            Self::Sky => "sky",
            Self::Horizon => "horizon",
            Self::Solar => "solar",
            Self::GlobeEarth => "earth-sphere",
            Self::GlobeMars => "mars-sphere",
            Self::Earth => "earth",
            Self::Mars => "mars",
            Self::Galaxy => "galaxy",
            Self::ObjectBrowser => "object-browser",
            Self::SpacecraftPaths => "spacecraft-paths",
        }
    }
}
pub struct Smoke {
    scenes: Vec<Scene>,
    index: usize,
    entered: bool,
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
                    Scene::Horizon,
                    Scene::Solar,
                    Scene::GlobeEarth,
                    Scene::GlobeMars,
                    Scene::Earth,
                    Scene::Mars,
                    Scene::Galaxy,
                    Scene::ObjectBrowser,
                    Scene::SpacecraftPaths,
                ],
                index: 0,
                entered: false,
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
            Scene::Sky => app.view = View::Sky,
            Scene::Horizon => app.view = View::Horizon,
            Scene::Solar => {
                // 単なるエラー文を描画成功として数えない。
                app.solar.prepare(&data.ephemeris, app.jd)?;
                app.view = View::Solar;
            }
            Scene::GlobeEarth | Scene::GlobeMars => {
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
                if !data.root.join("star_names_HIP.dat").try_exists()? {
                    return Ok(false);
                }
                app.objects.inspect_sirius(data)?;
                app.view = View::Sky;
            }
            Scene::Galaxy => {
                if !data.root.join("ourgalaxy_model.dat").try_exists()? {
                    eprintln!("SKIP galaxy（天の川モデルがありません）");
                    return Ok(false);
                }
                app.galaxies = Some(galaxies::Browser::new(data.root.clone(), ctx));
                app.view = View::Galaxy;
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
        if scene == Scene::Galaxy {
            let Some(g) = &app.galaxies else {
                return Ok(false);
            };
            if !g.ready()? {
                return Ok(false);
            }
        }
        if scene == Scene::ObjectBrowser && app.selected != Some(32349) {
            return Err(io::Error::other(
                "シリウスに対応するHIP 32349へ視点が移りません",
            ));
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
