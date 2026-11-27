use crate::{App, J2000, UNIX_EPOCH_JD};
use chrono::Utc;
use eframe::egui;

impl App {
    pub(super) fn submit_julian_input(&mut self) {
        let result = self
            .jd_input
            .trim()
            .parse::<f64>()
            .map_err(|_| "JD は数値で指定してください".to_owned())
            .and_then(|jd| self.apply_date(jd));
        match result {
            Ok(()) => self.playback.stop(),
            Err(message) => {
                self.jd_input = format!("{:.6}", self.jd);
                self.error = Some(message);
            }
        }
    }

    pub(super) fn settings_windows(&mut self, ctx: &egui::Context) {
        let mut open = self.time_settings;
        egui::Window::new("日時設定")
            .open(&mut open)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("UTC");
                    ui.text_edit_singleline(&mut self.utc);
                    if ui.button("適用").clicked() {
                        if let Some(jd) = crate::parse_utc(&self.utc) {
                            self.set_date(jd);
                        } else {
                            self.error =
                                Some("UTC は YYYY-MM-DD HH:MM:SS で指定してください".into());
                        }
                    }
                });
                ui.add_enabled_ui(self.data.is_some() && !self.playback.is_running(), |ui| {
                    ui.horizontal(|ui| {
                        ui.label("JD");
                        let entry = ui.text_edit_singleline(&mut self.jd_input);
                        let enter =
                            entry.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                        if ui.button("JDを適用").clicked() || enter {
                            self.submit_julian_input();
                        }
                    });
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
                });
                ui.separator();
                self.clock_controls(ui);
            });
        self.time_settings = open;
        let mut open = self.place_settings;
        egui::Window::new("観測地設定")
            .open(&mut open)
            .show(ctx, |ui| {
                let lat_changed = ui
                    .add(egui::Slider::new(&mut self.latitude, -90. ..=90.).text("緯度 °"))
                    .changed();
                let lon_changed = ui
                    .add(egui::Slider::new(&mut self.longitude, -180. ..=180.).text("東経 °"))
                    .changed();
                if lat_changed || lon_changed {
                    self.options.layers.observer.clear();
                }
                self.options.layers.place_ui(
                    ui,
                    self.data.as_ref(),
                    &mut self.latitude,
                    &mut self.longitude,
                );
            });
        self.place_settings = open;
        let mut open = self.settings;
        egui::Window::new("表示設定")
            .open(&mut open)
            .show(ctx, |ui| {
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
                self.picking.ui(ui);
                self.objects.display.ui(ui);
                self.options.layers.ui(ui, self.data.as_ref());
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
        self.settings = open;
    }
}
