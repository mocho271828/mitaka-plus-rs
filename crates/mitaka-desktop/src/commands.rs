use eframe::egui;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    Media,
    RunScript,
    ScriptPanel,
    LocalHtml,
    Open,
    LoadState,
    SaveState,
    Sky,
    Horizon,
    Solar,
    SolarControls,
    Land,
    Surface,
    Galaxy,
    Export,
    Search,
    Settings,
    Reset,
    Info,
}

const FILE: &[(Command, &str)] = &[
    (Command::Media, "画像・音声・動画を開く…"),
    (Command::Open, "データを開く…"),
    (Command::LoadState, "表示状態を開く…"),
    (Command::SaveState, "表示状態を保存…"),
    (Command::Export, "画像を書き出す…"),
];
const DISPLAY: &[(Command, &str)] = &[
    (Command::Sky, "全天図"),
    (Command::Horizon, "地平図"),
    (Command::Solar, "太陽系 3D"),
    (Command::Galaxy, "銀河カタログ…"),
    (Command::Search, "恒星を検索…"),
    (Command::Info, "選択天体の情報…"),
];
const SETTINGS: &[(Command, &str)] = &[
    (Command::RunScript, "Lua ファイルを実行…"),
    (Command::ScriptPanel, "Lua の実行・停止…"),
    (Command::LocalHtml, "ローカル HTML を開く…"),
    (Command::Settings, "観測・表示設定…"),
    (Command::Reset, "視点をリセット"),
    (Command::SolarControls, "天体を選択・飛行…"),
    (Command::Land, "離着陸 (S)"),
    (Command::Surface, "観測地点の地形を表示…"),
];
const GROUPS: &[(&str, &[(Command, &str)])] =
    &[("ファイル", FILE), ("表示", DISPLAY), ("操作", SETTINGS)];

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
                let item = muda::MenuItem::with_id(format!("{command:?}"), label, true, None);
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
