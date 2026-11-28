//! Mitaka_plus f9fe624 AppController::loadData の非同期データ切替。
use super::{objects::selections::Selections, projection::View, App, SkyData};
use eframe::egui;
use std::{io, path::PathBuf, sync::mpsc};

type Loaded = (SkyData, Selections);

pub(super) struct Request {
    root: PathBuf,
    result: mpsc::Receiver<io::Result<Loaded>>,
}

impl Request {
    fn start(root: PathBuf, selections: Selections, ctx: &egui::Context) -> io::Result<Self> {
        let (tx, result) = mpsc::channel();
        let path = root.clone();
        let ctx = ctx.clone();
        std::thread::Builder::new()
            .name("load-sky-data".into())
            .spawn(move || {
                let loaded =
                    SkyData::open(&path).and_then(|data| Ok((data, selections.rebind(&path)?)));
                if tx.send(loaded).is_ok() {
                    ctx.request_repaint();
                }
            })?;
        Ok(Self { root, result })
    }

    fn poll(&self) -> Option<io::Result<Loaded>> {
        match self.result.try_recv() {
            Ok(data) => Some(data),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => {
                Some(Err(io::Error::other("データ読込スレッドが終了しました")))
            }
        }
    }
}

impl App {
    pub(super) fn open(&mut self, ctx: &egui::Context) {
        if let Some(root) = rfd::FileDialog::new()
            .set_title("hip.dat と cf96.dat のあるフォルダを開く")
            .pick_folder()
        {
            self.load_data(root, ctx);
        }
    }

    pub(super) fn load_data(&mut self, root: PathBuf, ctx: &egui::Context) {
        // 残った IPC 要求も受信端ごと破棄し、旧スクリプトの変更を次のデータへ持ち越さない。
        self.scripts.cancel();
        self.playback.stop();
        self.motion.stop_motion(&mut self.solar);
        self.objects.stop_tracking();
        self.options.layers.stop_brightness();
        self.capture = None;
        self.pending_data = None;
        match Request::start(root, self.objects.selections.clone(), ctx) {
            Ok(request) => self.pending_data = Some(request),
            Err(err) => self.error = Some(err.to_string()),
        }
    }

    pub(super) fn poll_data(&mut self) {
        let Some(request) = &self.pending_data else {
            return;
        };
        let Some(result) = request.poll() else { return };
        match result {
            Ok((data, selections)) => self.install_data(data, selections),
            Err(err) => self.error = Some(format!("{}\n{err}", request.root.display())),
        }
        self.pending_data = None;
    }

    fn install_data(&mut self, data: SkyData, selections: Selections) {
        let jd = data.initial_date(self.jd);
        self.options.layers.observer.clear();
        if let Some(previous) = &self.data {
            self.options
                .layers
                .rebind_stars(&previous.layers, &data.layers);
        }
        self.options
            .layers
            .exceptions
            .retain(|id| data.constellations.iter().any(|c| c.abbreviation == *id));
        self.media.reset(&data.root);
        self.data = Some(data);
        self.set_date(jd);
        self.motion = Default::default();
        let display = std::mem::take(&mut self.objects.display);
        self.objects = Default::default();
        self.objects.display = display;
        self.objects.selections = selections;
        let orbits = self.solar.orbits;
        self.solar = Default::default();
        self.solar.orbits = orbits;
        self.surface = None;
        self.galaxies = None;
        self.background_render = Default::default();
        self.landscape_render = Default::default();
        if self.view == View::Surface {
            self.view = View::Sky;
        }
        self.selected = None;
        self.error = None;
        self.search_message.clear();
        self.restore_preferences();
    }
}

// GUIスモークで実ヘルパー、時計、ワーカーを通常の切替入口に通す。
pub(super) fn check_switch(app: &mut App, ctx: &egui::Context) -> io::Result<()> {
    fn finish(app: &mut App) -> io::Result<()> {
        let end = std::time::Instant::now() + std::time::Duration::from_secs(60);
        while app.pending_data.is_some() {
            if std::time::Instant::now() >= end {
                return Err(io::Error::other("データ切替がタイムアウトしました"));
            }
            app.poll_data();
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        Ok(())
    }
    let root = app
        .data
        .as_ref()
        .ok_or_else(|| io::Error::other("データがありません"))?
        .root
        .clone();
    let jd = app.jd;
    let missing = tempfile::tempdir()?;
    app.options.layers.selected = Some(Default::default());
    let exception = app
        .data
        .as_ref()
        .and_then(|d| d.constellations.first())
        .map(|c| c.abbreviation.clone());
    if let Some(id) = &exception {
        app.options.layers.exceptions.insert(id.clone());
    }
    app.solar.orbits = false;
    app.scripts.start_switch_probe(ctx)?;
    app.time_action(74, &serde_json::json!({"number": 1}))
        .map_err(io::Error::other)?;
    app.load_data(missing.path().to_owned(), ctx);
    finish(app)?;
    if app.error.take().is_none()
        || app.data.as_ref().is_none_or(|d| d.root != root)
        || app.playback.plus_speed != 0
    {
        return Err(io::Error::other(
            "失敗したデータ切替で直前のデータと停止状態を保持できません",
        ));
    }
    app.time_action(74, &serde_json::json!({"number": 1}))
        .map_err(io::Error::other)?;
    app.load_data(missing.path().to_owned(), ctx);
    app.load_data(root.clone(), ctx);
    finish(app)?;
    app.clock_frame(ctx);
    app.scripts_frame(ctx);
    if app.error.is_some()
        || app.solar.orbits
        || app
            .options
            .layers
            .selected
            .as_ref()
            .is_none_or(|stars| !stars.is_empty())
        || exception
            .as_ref()
            .is_some_and(|id| !app.options.layers.exceptions.contains(id))
        || app.jd != jd
        || app.playback.plus_speed != 0
        || app.data.as_ref().is_none_or(|d| d.root != root)
    {
        return Err(io::Error::other(
            "古いデータ読込、Luaまたは時計が新データに作用しました",
        ));
    }
    app.options.layers.selected = None;
    app.options.layers.exceptions.clear();
    app.solar.orbits = true;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_superseded_request_cannot_deliver_its_result() {
        let (old, old_rx) = mpsc::channel();
        let (latest, latest_rx) = mpsc::channel();
        let mut request = Request {
            root: "first".into(),
            result: old_rx,
        };
        assert!(request.poll().is_none());
        request = Request {
            root: "second".into(),
            result: latest_rx,
        };
        assert!(old.send(Err(io::Error::other("old"))).is_err());
        latest.send(Err(io::Error::other("latest"))).unwrap();
        assert_eq!(request.poll().unwrap().err().unwrap().to_string(), "latest");
    }
}
