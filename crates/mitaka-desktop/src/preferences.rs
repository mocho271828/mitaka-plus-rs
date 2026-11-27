//! Mitaka_plus f9fe624 AppSessionCommands::displayPreferences の通常終了時保存。
use super::{session::Session, session_ui::DisplayLayers, App};
use std::{io, path::PathBuf};

pub(super) fn config_dir() -> io::Result<PathBuf> {
    dirs::config_dir()
        .map(|dir| dir.join("mitaka-plus-rs"))
        .ok_or_else(|| io::Error::other("アプリ設定の保存先を特定できません"))
}

#[derive(Default)]
pub(super) enum Preferences {
    #[default]
    Disabled,
    Unread(PathBuf),
    Pending(PathBuf, Session),
    Ready(PathBuf),
}

impl App {
    pub(super) fn restore_preferences(&mut self) {
        let Some(data) = self.data.as_ref() else {
            return;
        };
        let (path, state) = match std::mem::take(&mut self.preferences) {
            Preferences::Unread(path) => match Session::read(&path) {
                Ok(state) => (path, state),
                Err(e) if e.kind() == io::ErrorKind::NotFound => {
                    self.preferences = Preferences::Ready(path);
                    return;
                }
                Err(e) => {
                    self.error = Some(format!("保存された表示設定を読み込めません: {e}"));
                    return;
                }
            },
            Preferences::Pending(path, state) => (path, state),
            other => {
                self.preferences = other;
                return;
            }
        };
        match DisplayLayers::prepare(&state, data) {
            Ok(display) => {
                self.apply_display_settings(&state.0, display);
                self.preferences = Preferences::Ready(path);
            }
            Err(e) => {
                self.error = Some(format!("現在のデータに表示設定を適用できません: {e}"));
                self.preferences = Preferences::Pending(path, state);
            }
        }
    }

    pub(super) fn save_preferences(&self) -> io::Result<()> {
        let (Preferences::Ready(path), Some(data)) = (&self.preferences, &self.data) else {
            return Ok(());
        };
        if self.pending_data.is_some() {
            return Ok(());
        }
        let dir = path
            .parent()
            .ok_or_else(|| io::Error::other("表示設定の保存先がありません"))?;
        if !dir.is_dir() {
            // XDG_CONFIG_HOMEなどがデータ内を指していても、元データの中にフォルダを作らない。
            let ancestor = dir
                .ancestors()
                .find(|p| p.is_dir())
                .ok_or_else(|| io::Error::other("表示設定の親ディレクトリがありません"))?;
            super::capture::output_path(&ancestor.join("display-v9.json"), &data.root)?;
            std::fs::create_dir_all(dir)?;
        }
        let file = super::capture::output_path(path, &data.root)?;
        let mut state = Session::default();
        self.write_display_settings(&mut state.0, data);
        // 観測地は明示的に保存するセッションにだけ含まれる。
        state.0["observer_place_identifier_base64"] = "".into();
        state.write(&file)
    }
}

// 実際の保存・復元経路を隔離した設定ディレクトリで通す。スモークは利用者の設定に触れない。
pub(super) fn check_restart(app: &mut App) -> io::Result<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("settings/display-v9.json");
    let data = app
        .data
        .as_ref()
        .ok_or_else(|| io::Error::other("データがありません"))?;
    let mut before = Session::default();
    app.write_display_settings(&mut before.0, data);
    let flags = |app: &App| {
        [
            app.options.lines,
            app.options.layers.equatorial,
            app.playback.show,
        ]
    };
    let set_flags = |app: &mut App, [lines, equatorial, time]: [bool; 3]| {
        app.options.lines = lines;
        app.options.layers.equatorial = equatorial;
        app.playback.show = time;
    };
    let opposite = flags(app);
    let expected = opposite.map(|value| !value);
    app.preferences = Preferences::Unread(path.clone());
    app.restore_preferences();
    set_flags(app, expected);
    app.save_preferences()?;
    let saved = std::fs::read(&path)?;
    let remembered = path.with_file_name("remembered-v9.json");
    std::fs::write(&remembered, b"remembered")?;
    set_flags(app, opposite);
    let lat = app.latitude;
    app.latitude = 12.5;
    app.preferences = Preferences::Unread(path.clone());
    let loaded = app.data.take();
    app.restore_preferences();
    if flags(app) != opposite {
        return Err(io::Error::other(
            "データ読込より前に表示設定が変更されました",
        ));
    }
    app.data = loaded;
    app.restore_preferences();
    if flags(app) != expected || app.latitude != 12.5 {
        return Err(io::Error::other(
            "表示設定の再読込で表示・観測地点が期待値と一致しません",
        ));
    }
    app.latitude = lat;
    let mut mismatch = Session::decode(&saved)?;
    mismatch.0["selected_star_name_hips"] = serde_json::json!([i32::MAX]);
    mismatch.write(&path)?;
    let incompatible = std::fs::read(&path)?;
    set_flags(app, opposite);
    let manual = directory.path().join("session.json");
    app.save_session(&manual)?;
    for (original, can_retry) in [
        (incompatible.as_slice(), true),
        (b"invalid json" as &[u8], false),
    ] {
        std::fs::write(&path, original)?;
        app.preferences = Preferences::Unread(path.clone());
        app.restore_preferences();
        app.save_preferences()?;
        if app.error.take().is_none() || flags(app) != opposite || std::fs::read(&path)? != original
        {
            return Err(io::Error::other(
                "適用できない設定を部分的に復元、または上書きしました",
            ));
        }
        if can_retry {
            let names = &mut app.data.as_mut().unwrap().layers.known_star_names;
            names.insert(i32::MAX);
            app.restore_preferences();
            app.data
                .as_mut()
                .unwrap()
                .layers
                .known_star_names
                .remove(&i32::MAX);
            if flags(app) != expected {
                return Err(io::Error::other(
                    "対応カタログを開いた後も設定が保留されています",
                ));
            }
            set_flags(app, opposite);
            app.preferences = Preferences::Unread(path.clone());
            app.restore_preferences();
            if app.restore_session(&path).is_ok() {
                return Err(io::Error::other("不一致のセッションを復元しました"));
            }
            app.save_preferences()?;
            if std::fs::read(&path)? != original {
                return Err(io::Error::other("手動復元の失敗で保留設定を上書きしました"));
            }
        }
        app.restore_session(&manual)?;
        app.save_preferences()?;
        if can_retry {
            let restored = Session::read(&path)?;
            if restored.0["constellations"]["lines"].as_bool() != Some(opposite[0])
                || restored.0["selected_star_name_hips"] == mismatch.0["selected_star_name_hips"]
            {
                return Err(io::Error::other(
                    "明示復元後も古い表示設定が保留されています",
                ));
            }
        } else if std::fs::read(&path)? != original {
            return Err(io::Error::other(
                "手動復元で破損した表示設定を上書きしました",
            ));
        }
    }
    if std::fs::read(&remembered)? != b"remembered" {
        return Err(io::Error::other(
            "自動保存が記憶済みセッションを上書きしました",
        ));
    }
    let data = app.data.as_ref().unwrap();
    let layers = DisplayLayers::prepare(&before, data)?;
    app.apply_display_settings(&before.0, layers);
    app.preferences = Preferences::Disabled;
    Ok(())
}
