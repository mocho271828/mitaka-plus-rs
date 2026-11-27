//! Mitaka_plus f9fe624 AppController::loadData の非同期データ切替。
use super::{projection::View, App, SkyData};
use eframe::egui;
use std::{io, path::PathBuf, sync::mpsc};

pub(super) struct Request {
    root: PathBuf,
    result: mpsc::Receiver<io::Result<SkyData>>,
}

impl Request {
    fn start(root: PathBuf, ctx: &egui::Context) -> io::Result<Self> {
        let (tx, result) = mpsc::channel();
        let path = root.clone();
        let ctx = ctx.clone();
        std::thread::Builder::new()
            .name("load-sky-data".into())
            .spawn(move || {
                if tx.send(SkyData::open(&path)).is_ok() {
                    ctx.request_repaint();
                }
            })?;
        Ok(Self { root, result })
    }

    fn poll(&self) -> Option<io::Result<SkyData>> {
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
        match Request::start(root, ctx) {
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
            Ok(data) => self.install_data(data),
            Err(err) => self.error = Some(format!("{}\n{err}", request.root.display())),
        }
        self.pending_data = None;
    }

    fn install_data(&mut self, data: SkyData) {
        let jd = data.initial_date(self.jd);
        self.options.layers.observer.clear();
        self.options.layers.selected = None;
        self.options.layers.exceptions.clear();
        self.media.reset(&data.root);
        self.data = Some(data);
        self.set_date(jd);
        self.motion = Default::default();
        self.objects = Default::default();
        self.solar = Default::default();
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
        || app.jd != jd
        || app.playback.plus_speed != 0
        || app.data.as_ref().is_none_or(|d| d.root != root)
    {
        return Err(io::Error::other(
            "古いデータ読込、Luaまたは時計が新データに作用しました",
        ));
    }
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
