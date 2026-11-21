pub mod catalog;
mod grid;
pub(crate) mod landscape;
mod render;
mod state;
mod stellar;

use catalog::{decode, LayersCatalog};
use eframe::egui;
use std::collections::{BTreeSet, HashSet};

pub struct LayerOptions {
    pub equatorial: bool,
    pub ecliptic: bool,
    pub galactic: bool,
    pub horizontal: bool,
    pub boundaries: bool,
    pub illustrations: bool,
    pub stars: bool,
    pub proper_names: bool,
    pub selected_only: bool,
    pub bayer: bool,
    pub flamsteed: bool,
    pub hip: bool,
    pub brightness: f32,
    pub marking: u8,
    pub landscape: bool,
    pub observer: Vec<u8>,
    pub selected: Option<BTreeSet<i32>>,
    pub show_all_constellations: bool,
    pub exceptions: HashSet<Vec<u8>>,
}
impl Default for LayerOptions {
    fn default() -> Self {
        Self {
            equatorial: false,
            ecliptic: false,
            galactic: false,
            horizontal: false,
            boundaries: false,
            illustrations: false,
            stars: true,
            proper_names: false,
            selected_only: false,
            bayer: false,
            flamsteed: false,
            hip: false,
            brightness: 1.,
            marking: 0,
            landscape: true,
            observer: vec![],
            selected: None,
            show_all_constellations: true,
            exceptions: HashSet::new(),
        }
    }
}
impl LayerOptions {
    pub fn shows(&self, id: &[u8]) -> bool {
        self.show_all_constellations != self.exceptions.contains(id)
    }
    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        data: Option<&crate::catalog::SkyData>,
        latitude: &mut f64,
        longitude: &mut f64,
    ) {
        if let Some(data) = data {
            let title = data
                .layers
                .places
                .places
                .iter()
                .find(|p| p.id == self.observer)
                .map(|p| decode(&p.name))
                .unwrap_or("緯度・経度を入力".into());
            egui::ComboBox::from_label("観測地点")
                .selected_text(title)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.observer, vec![], "緯度・経度を入力");
                    for place in &data.layers.places.places {
                        let row = format!("{}  ({})", decode(&place.name), decode(&place.id));
                        if ui
                            .add_enabled(
                                place.earth(),
                                egui::SelectableLabel::new(self.observer == place.id, row),
                            )
                            .clicked()
                        {
                            self.observer = place.id.clone();
                            *latitude = place.latitude;
                            *longitude = place.longitude_signed();
                        }
                    }
                });
        }
        ui.checkbox(&mut self.landscape, "観測地点のパノラマ");
        ui.checkbox(&mut self.landscape, "観測地点のパノラマ");
        ui.collapsing("座標線・星座", |ui| {
            for (setting, name) in [
                (&mut self.equatorial, "赤道座標"),
                (&mut self.ecliptic, "黄道"),
                (&mut self.galactic, "銀河座標"),
                (&mut self.horizontal, "地平座標"),
                (&mut self.boundaries, "星座境界"),
                (&mut self.illustrations, "星座絵"),
            ] {
                ui.checkbox(setting, name);
            }
            if let Some(data) = data {
                ui.collapsing("表示する星座", |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("constellation selection")
                        .max_height(140.)
                        .show(ui, |ui| {
                            for record in &data.constellations {
                                let mut value = self.shows(&record.abbreviation);
                                if ui
                                    .checkbox(&mut value, decode(&record.japanese_hiragana_bytes))
                                    .changed()
                                {
                                    if value == self.show_all_constellations {
                                        self.exceptions.remove(&record.abbreviation);
                                    } else {
                                        self.exceptions.insert(record.abbreviation.clone());
                                    }
                                }
                            }
                        });
                });
            }
        });
        ui.collapsing("恒星", |ui| {
            for (flag, name) in [
                (&mut self.stars, "恒星を表示"),
                (&mut self.proper_names, "固有名"),
                (&mut self.selected_only, "選択した固有名のみ"),
                (&mut self.bayer, "バイエル符号"),
                (&mut self.flamsteed, "フラムスティード番号"),
                (&mut self.hip, "HIP番号"),
            ] {
                ui.checkbox(flag, name);
            }
            ui.add(
                egui::Slider::new(&mut self.brightness, 0.01..=100.)
                    .logarithmic(true)
                    .clamping(egui::SliderClamping::Edits)
                    .text("明るさ係数（小さいほど明るい）"),
            );
            egui::ComboBox::from_label("マーカー")
                .selected_text(["なし", "二重星", "系外惑星のある星"][self.marking as usize])
                .show_ui(ui, |ui| {
                    for (i, text) in ["なし", "二重星", "系外惑星のある星"].iter().enumerate()
                    {
                        ui.selectable_value(&mut self.marking, i as u8, *text);
                    }
                });
            if let Some(data) = data {
                ui.collapsing("固有名を選択", |ui| {
                    let selection = self
                        .selected
                        .get_or_insert_with(|| data.layers.featured.iter().copied().collect());
                    let mut entries: Vec<_> = data.layers.names.iter().collect();
                    entries.sort_unstable_by_key(|(hip, _)| *hip);
                    egui::ScrollArea::vertical()
                        .id_salt("name selection")
                        .max_height(140.)
                        .show(ui, |ui| {
                            for (&hip, name) in entries {
                                let mut on = selection.contains(&hip);
                                if ui.checkbox(&mut on, format!("{name}  HIP {hip}")).changed() {
                                    if on {
                                        selection.insert(hip);
                                    } else {
                                        selection.remove(&hip);
                                    }
                                }
                            }
                        });
                });
            }
        });
        if let Some(data) = data {
            if !data.layers.warnings.is_empty() {
                ui.collapsing("追加データの読込結果", |ui| {
                    for w in &data.layers.warnings {
                        ui.label(w);
                    }
                });
            }
        }
    }
    pub fn includes_name(&self, hip: i32, c: &LayersCatalog) -> bool {
        !self.selected_only
            || self
                .selected
                .as_ref()
                .map_or_else(|| c.featured.contains(&hip), |s| s.contains(&hip))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        catalog::SkyData,
        chart::{Chart, Options},
        projection::{Camera, View},
        session::Session,
    };
    use mitaka_core::astronomy::{ObservationFrame, J2000};
    use serde_json::json;
    #[test]
    fn changes_flags_brightness_and_snapshot_together() {
        let mut o = LayerOptions::default();
        assert!(o.action(31, &json!({"enabled":true})).unwrap());
        assert!(o.action(61, &json!({})).unwrap());
        assert!(o.action(60, &json!({"number":0})).is_err());
        let reply = o.snapshot(json!({"ok":true}));
        assert_eq!(reply["equatorial"], true);
        assert!(reply["starBrightness"].as_f64().unwrap() < 1.);
        assert!(!o.action(12345, &json!({})).unwrap());
        let (radius, opacity) = o.point_style(0.);
        assert!(radius > 2.8 && opacity == 1.);
    }
    #[test]
    #[ignore = "requires a local Plus 1.5.4 data directory in MITAKA_DATA_ROOT"]
    fn real_catalog_layers_render_and_round_trip() {
        let root = std::env::var_os("MITAKA_DATA_ROOT").expect("MITAKA_DATA_ROOT");
        let data = SkyData::open(std::path::Path::new(&root)).unwrap();
        let c = &data.layers;
        println!(
            "CAB={} names={} featured={} places={} warnings={:?}",
            c.art.len(),
            c.names.len(),
            c.featured.len(),
            c.places.places.len(),
            c.warnings
        );
        assert_eq!(c.art.len(), 89);
        assert!(c.names.len() > 100);
        assert!(c.featured.len() > 10);
        assert_eq!(c.names[&32349], "シリウス");
        // f9fe624 の load_constellation_art → constellation_art_direction → equatorial_coordinates 出力。
        let orion = mitaka_core::constellation_art::load(
            &std::path::Path::new(&root).join("constellations/ori.cab"),
            Default::default(),
        )
        .unwrap();
        assert_eq!(orion.coordinates.len(), 2078);
        assert_eq!(orion.indices.len(), 2601);
        for (i, ra, dec) in [
            (0, 88.1903605276116, -8.513635347046629),
            (1039, 84.07841024666675, 8.971802270477953),
            (2077, 94.0003940921095, 13.749351529326939),
        ] {
            let [x, y] = orion.coordinates[i];
            assert!((x - ra).abs() < 1e-10 && (y - dec).abs() < 1e-10);
        }
        let site = c.places.places.iter().find(|p| p.earth()).unwrap();
        let mut options = Options::default();
        options.layers.observer = site.id.clone();
        options.layers.boundaries = true;
        options.layers.illustrations = true;
        options.layers.proper_names = true;
        options.layers.ecliptic = true;
        options.layers.galactic = true;
        options.layers.equatorial = true;
        options.layers.horizontal = true;
        let mut session = Session::default();
        let value = &mut session.0;
        value["observer_latitude_deg"] = json!(site.latitude);
        value["observer_longitude_deg"] = json!(site.longitude_signed());
        options.layers.save(value, &data);
        let restored = LayerOptions::restore(value, &data).unwrap();
        assert!(restored.illustrations && restored.galactic);
        assert_eq!(restored.observer, site.id);
        let frame = ObservationFrame::new(site.latitude, site.longitude_signed(), J2000).unwrap();
        let ctx = egui::Context::default();
        let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800., 480.));
        for view in [View::Sky, View::Horizon] {
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(rect),
                    ..Default::default()
                },
                |ctx| {
                    let painter = ctx.layer_painter(egui::LayerId::background());
                    let chart = Chart {
                        view,
                        camera: Camera::default(),
                        frame: &frame,
                        rect,
                    };
                    chart.paint(&painter, &data, &options, None, None);
                },
            );
            assert!(
                output.shapes.len() > 20000,
                "projected shapes: {}",
                output.shapes.len()
            );
            assert!(!ctx
                .tessellate(output.shapes, output.pixels_per_point)
                .is_empty());
        }
        value["observer_latitude_deg"] = json!(site.latitude + 1.);
        assert!(LayerOptions::restore(value, &data).is_err());
    }
}
