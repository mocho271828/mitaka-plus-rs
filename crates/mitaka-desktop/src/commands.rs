use eframe::egui;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    Media,
    MediaList,
    Projector,
    Fullscreen,
    About,
    RunScript,
    ScriptPanel,
    LocalHtml,
    Open,
    Quit,
    ZoomIn,
    ZoomOut,
    RememberState,
    RecallState,
    LoadState,
    SaveWindow,
    RecallWindow,
    SaveState,
    Sky,
    Horizon,
    Solar,
    SolarControls,
    Journey,
    CameraControls,
    Land,
    Surface,
    Galaxy,
    Export,
    Objects,
    Search,
    Settings,
    TimeSettings,
    PlaceSettings,
    Reset,
    Info,
}

const FILE: &[(Command, &str)] = &[
    (Command::MediaList, "ローカル媒体の登録…"),
    (Command::Media, "画像・音声・動画を開く…"),
    (Command::Open, "データを開く…"),
    (Command::RememberState, "現在の表示状態を記憶"),
    (Command::RecallState, "記憶した表示状態を呼び出す"),
    (Command::LoadState, "表示状態を開く…"),
    (Command::SaveState, "表示状態を保存…"),
    (Command::Export, "画像を書き出す…"),
    #[cfg(not(target_os = "macos"))]
    (Command::Quit, "終了"),
];
const DISPLAY: &[(Command, &str)] = &[
    (Command::SaveWindow, "主ウィンドウの位置を記憶"),
    (Command::RecallWindow, "主ウィンドウの位置を復元"),
    (Command::Projector, "投影ウィンドウ…"),
    (Command::Fullscreen, "全画面表示を切り替える"),
    (Command::Sky, "全天図"),
    (Command::Horizon, "地平図"),
    (Command::Solar, "太陽系 3D"),
    (Command::Galaxy, "銀河カタログ…"),
    (Command::Objects, "天体ブラウザ…"),
    (Command::Search, "恒星を検索…"),
    (Command::Info, "選択天体の情報…"),
];
const SETTINGS: &[(Command, &str)] = &[
    (Command::RunScript, "Lua ファイルを実行…"),
    (Command::ScriptPanel, "Lua コンソール…"),
    (Command::About, "Mitaka Plus RS について…"),
    (Command::LocalHtml, "ローカル HTML を開く…"),
    (Command::TimeSettings, "日時設定…"),
    (Command::PlaceSettings, "観測地設定…"),
    (Command::Settings, "表示設定…"),
    (Command::Reset, "視点をリセット"),
    (Command::CameraControls, "カメラ設定…"),
    (Command::SolarControls, "天体を選択・飛行…"),
    (Command::Journey, "天体間飛行…"),
    (Command::Land, "離着陸 (S)"),
    (Command::Surface, "観測地点の地形を表示…"),
];
const GROUPS: &[(&str, &[(Command, &str)])] =
    &[("ファイル", FILE), ("表示", DISPLAY), ("操作", SETTINGS)];

#[cfg(target_os = "macos")]
pub fn native_shortcut(command: Command) -> Option<muda::accelerator::Accelerator> {
    use muda::accelerator::{Accelerator, Code, Modifiers};
    let (modifiers, key) = match command {
        Command::Open => (Modifiers::SUPER, Code::KeyO),
        Command::SaveState => (Modifiers::SUPER, Code::KeyS),
        Command::Objects => (Modifiers::SUPER, Code::KeyF),
        Command::Fullscreen => (Modifiers::CONTROL | Modifiers::SUPER, Code::KeyF),
        _ => return None,
    };
    Some(Accelerator::new(Some(modifiers), key))
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
pub struct Menus {
    _menu: muda::Menu,
    incoming: std::sync::mpsc::Receiver<Command>,
}
#[cfg(any(target_os = "macos", target_os = "windows"))]
impl Menus {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let menu = muda::Menu::new();
        #[cfg(target_os = "macos")]
        {
            let application = muda::Submenu::new("Mitaka Plus", true);
            application.append(&muda::PredefinedMenuItem::about(
                Some("Mitaka Plus について"),
                None,
            ))?;
            application.append(&muda::PredefinedMenuItem::quit(Some("Mitaka Plus を終了")))?;
            menu.append(&application)?;
        }
        for &(name, items) in GROUPS {
            let submenu = muda::Submenu::new(name, true);
            for &(command, label) in items {
                #[cfg(target_os = "macos")]
                let shortcut = native_shortcut(command);
                #[cfg(target_os = "windows")]
                let shortcut = None;
                let item = muda::MenuItem::with_id(format!("{command:?}"), label, true, shortcut);
                submenu.append(&item)?;
            }
            menu.append(&submenu)?;
        }
        #[cfg(target_os = "macos")]
        menu.init_for_nsapp();
        #[cfg(target_os = "windows")]
        {
            use raw_window_handle::{HasWindowHandle, RawWindowHandle};
            let RawWindowHandle::Win32(win) = cc.window_handle()?.as_raw() else {
                return Err("Win32 ウィンドウを取得できません".into());
            };
            // このHWNDはeframeの主ウィンドウが生存している期間にだけ使用する。
            unsafe {
                menu.init_for_hwnd(win.hwnd.get())?;
            }
        }
        let context = cc.egui_ctx.clone();
        let (sender, incoming) = std::sync::mpsc::channel();
        muda::MenuEvent::set_event_handler(Some(move |event: muda::MenuEvent| {
            for &(_, items) in GROUPS {
                if let Some(&(command, _)) =
                    items.iter().find(|(c, _)| format!("{c:?}") == event.id.0)
                {
                    let _ = sender.send(command);
                    context.request_repaint();
                    break;
                }
            }
        }));
        Ok(Self {
            _menu: menu,
            incoming,
        })
    }
    pub fn poll(&self, _ctx: &egui::Context) -> Vec<Command> {
        self.incoming.try_iter().collect()
    }
}
#[cfg(target_os = "linux")]
pub struct Menus;
#[cfg(target_os = "linux")]
impl Menus {
    pub fn new(
        _: &eframe::CreationContext<'_>,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        Ok(Self)
    }
    pub fn poll(&self, ctx: &egui::Context) -> Vec<Command> {
        let mut selected = vec![];
        egui::TopBottomPanel::top("menu").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                for &(name, items) in GROUPS {
                    ui.menu_button(name, |ui| {
                        for &(command, label) in items {
                            if ui.button(label).clicked() {
                                selected.push(command);
                                ui.close_menu();
                            }
                        }
                    });
                }
            })
        });
        selected
    }
}
