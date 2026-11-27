//! SkyCatalogRenderer.mm と同じ全件密度と星雲の視直径を全天図へ重ねる。
mod index;
use crate::{chart::Chart, projection::Camera};
use eframe::egui::{
    self, Align2, Color32, ColorImage, FontId, Painter, Rect, Stroke, TextureHandle,
};
use index::{density, Index};
use mitaka_core::extended_catalog::{load_ngc, load_sdss, NgcRecord, SdssKind};
use serde_json::Value;
use std::{io, path::Path, sync::mpsc};

#[derive(Default)]
pub(crate) struct Display {
    pub ngc: bool,
    pub galaxies: bool,
    pub quasars: bool,
}
const FLAGS: [(u64, &str, &str); 3] = [
    (9, "nebulae", "show_nebulae"),
    (10, "galaxies", "show_galaxies"),
    (11, "quasars", "show_quasars"),
];
impl Display {
    pub fn ui(&mut self, ui: &mut egui::Ui) {
        ui.collapsing("全天図の星雲・銀河", |ui| {
            ui.checkbox(&mut self.ngc, "NGC / IC");
            ui.checkbox(&mut self.galaxies, "SDSS 銀河");
            ui.checkbox(&mut self.quasars, "SDSS クエーサー");
        });
    }
    fn flags(&self) -> [bool; 3] {
        [self.ngc, self.galaxies, self.quasars]
    }
    pub fn flag(&mut self, id: u64) -> Option<&mut bool> {
        match id {
            9 => Some(&mut self.ngc),
            10 => Some(&mut self.galaxies),
            11 => Some(&mut self.quasars),
            _ => None,
        }
    }
    pub fn snapshot(&self, out: &mut Value) {
        for ((_, key, _), enabled) in FLAGS.into_iter().zip(self.flags()) {
            out[key] = Value::Bool(enabled);
        }
    }
    pub fn save(&self, out: &mut Value) {
        for ((_, _, key), enabled) in FLAGS.into_iter().zip(self.flags()) {
            out[key] = Value::Bool(enabled);
        }
    }
    pub fn restore(v: &Value) -> io::Result<Self> {
        let mut out = Self::default();
        for (id, _, key) in FLAGS {
            *out.flag(id).unwrap() = v[key]
                .as_bool()
                .ok_or_else(|| io::Error::other(format!("invalid {key}")))?;
        }
        Ok(out)
    }
}
#[derive(Default)]
enum Load<T> {
    #[default]
    Unloaded,
    Loading(mpsc::Receiver<io::Result<T>>),
    Loaded(T),
    Failed(String),
}
impl<T: Send + 'static> Load<T> {
    fn get(
        &mut self,
        ctx: &egui::Context,
        read: impl FnOnce() -> io::Result<T> + Send + 'static,
    ) -> Result<Option<&T>, &str> {
        match self {
            Self::Unloaded => {
                let (tx, rx) = mpsc::channel();
                let ctx = ctx.clone();
                std::thread::spawn(move || {
                    let _ = tx.send(read());
                    ctx.request_repaint();
                });
                *self = Self::Loading(rx);
            }
            Self::Loading(rx) => match rx.try_recv() {
                Ok(Ok(value)) => *self = Self::Loaded(value),
                Ok(Err(e)) => *self = Self::Failed(e.to_string()),
                Err(mpsc::TryRecvError::Disconnected) => {
                    *self = Self::Failed("カタログ読み込みが中断されました".to_owned())
                }
                Err(mpsc::TryRecvError::Empty) => {}
            },
            _ => {}
        }
        match self {
            Self::Failed(e) => Err(e),
            Self::Loaded(v) => Ok(Some(v)),
            _ => Ok(None),
        }
    }
}
#[derive(Clone, Copy, PartialEq)]
struct CacheKey {
    angles: [f64; 3],
    size: [usize; 2],
}
impl CacheKey {
    fn new(cam: Camera, rect: Rect) -> Self {
        Self {
            angles: [cam.longitude, cam.latitude, cam.zoom],
            size: [rect.width(), rect.height()].map(|d| (d / 4.).ceil().clamp(1., 4096.) as usize),
        }
    }
}
#[derive(Default)]
struct SdssLayer {
    rows: Load<Index<[f32; 2]>>,
    cache: Option<(CacheKey, TextureHandle)>,
}
impl SdssLayer {
    fn draw(
        &mut self,
        p: &Painter,
        chart: &Chart<'_>,
        root: &Path,
        kind: SdssKind,
    ) -> Result<bool, &str> {
        let (filename, rgb) = match kind {
            SdssKind::Galaxy => ("SDSS_GAL.dat", [0.94, 0.64, 0.30]),
            SdssKind::Quasar => ("SDSS_QSO.dat", [0.73, 0.50, 0.94]),
        };
        let path = root.join(filename);
        let Some(records) = self.rows.get(p.ctx(), move || {
            let rows = load_sdss(&path, kind)?;
            Ok(Index::new(rows.into_iter().map(|r| [r.ra_deg, r.dec_deg])))
        })?
        else {
            return Ok(false);
        };
        let key = CacheKey::new(chart.camera, chart.rect);
        if self.cache.as_ref().is_none_or(|(cached, _)| *cached != key) {
            let counts = density(records, chart.camera, key.size);
            let image = ColorImage {
                size: key.size,
                pixels: counts
                    .into_iter()
                    .map(|n| {
                        if n == 0 {
                            return Color32::TRANSPARENT;
                        }
                        let a = (60. + 50. * f64::from(n).ln_1p()).min(220.) as u8;
                        let [r, g, b] = rgb.map(|f| (f * f64::from(a)) as u8);
                        Color32::from_rgba_premultiplied(r, g, b, a)
                    })
                    .collect(),
            };
            let texture = p
                .ctx()
                .load_texture(filename, image, egui::TextureOptions::NEAREST);
            self.cache = Some((key, texture));
        }
        if let Some((_, t)) = &self.cache {
            p.image(
                t.id(),
                chart.rect,
                Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1., 1.)),
                Color32::WHITE,
            );
        }
        Ok(true)
    }
}
#[derive(Default)]
pub(crate) struct Scene {
    ngc: Load<Index<NgcRecord>>,
    galaxies: SdssLayer,
    quasars: SdssLayer,
}
impl Scene {
    fn ngc(
        &mut self,
        p: &Painter,
        chart: &Chart<'_>,
        root: &Path,
        limit: f32,
    ) -> Result<bool, &str> {
        let path = root.join("ngc.dat");
        let Some(records) = self
            .ngc
            .get(p.ctx(), move || Ok(Index::new(load_ngc(&path)?)))?
        else {
            return Ok(false);
        };
        let stroke = Stroke::new(0.7_f32, Color32::from_rgba_unmultiplied(97, 214, 181, 204));
        records.visible(chart.camera, |r, [x, y]| {
            if r.magnitude != 0. && r.magnitude > limit {
                return;
            }
            let radius =
                f64::from(r.size_arcmin) * f64::from(chart.rect.width()) * chart.camera.zoom
                    / (60. * 360. * 2.);
            let pos = chart.rect.min
                + egui::vec2(
                    x as f32 * chart.rect.width(),
                    y as f32 * chart.rect.height(),
                );
            p.circle_stroke(pos, radius.clamp(2., 20.) as f32, stroke);
        });
        Ok(true)
    }
    pub fn draw(
        &mut self,
        display: &Display,
        p: &Painter,
        chart: &Chart<'_>,
        root: &Path,
        limit: f32,
    ) {
        let mut messages = vec![];
        let mut message = |result: Result<bool, &str>, file: &str| match result {
            Ok(true) => {}
            Ok(false) => messages.push(format!("{file} を読み込み中")),
            Err(e) => messages.push(format!("{file}: {e}")),
        };
        if display.galaxies {
            message(
                self.galaxies.draw(p, chart, root, SdssKind::Galaxy),
                "SDSS 銀河",
            );
        }
        if display.quasars {
            message(
                self.quasars.draw(p, chart, root, SdssKind::Quasar),
                "SDSS クエーサー",
            );
        }
        if display.ngc {
            message(self.ngc(p, chart, root, limit), "NGC / IC");
        }
        if display.galaxies || display.quasars {
            messages.push("SDSS は可視域の全件を約4画素単位で密度表示（等級情報なし）".into());
        }
        for (i, m) in messages.iter().enumerate() {
            p.text(
                chart.rect.left_bottom() + egui::vec2(8., -5. - 14. * i as f32),
                Align2::LEFT_BOTTOM,
                m,
                FontId::proportional(11.),
                Color32::LIGHT_GRAY,
            );
        }
    }
    pub fn ready(&self) -> io::Result<bool> {
        fn ready<T>(state: &Load<T>) -> io::Result<bool> {
            match state {
                Load::Failed(e) => Err(io::Error::other(e.clone())),
                Load::Loaded(_) => Ok(true),
                _ => Ok(false),
            }
        }
        Ok(ready(&self.ngc)? && ready(&self.galaxies.rows)? && ready(&self.quasars.rows)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn all_display_switches_round_trip_across_lua_and_v9() {
        let mut opts = crate::layers::LayerOptions::default();
        let mut session = crate::session::Session::default();
        for id in 9..=11 {
            assert!(opts
                .action(id, &serde_json::json!({"enabled":true}))
                .unwrap());
        }
        let reply = opts.snapshot(serde_json::json!({}));
        opts.deep_sky.save(&mut session.0);
        for (_, name, key) in FLAGS {
            assert_eq!(reply[name], true);
            assert_eq!(session.0[key], true);
        }
        assert_eq!(Display::restore(&session.0).unwrap().flags(), [true; 3]);
        session.0["show_quasars"] = Value::String("true".into());
        assert!(Display::restore(&session.0).is_err());
    }
    #[test]
    #[ignore = "MITAKA_DATA_ROOT にオリジナルのデータを指定してC++原版による全画素カウントと比較"]
    fn shipped_bins_match_native_catalog_renderer() {
        let root = std::path::PathBuf::from(std::env::var_os("MITAKA_DATA_ROOT").unwrap());
        let cameras = [(180., 0., 1.), (359., 0., 3.), (185., 29., 16.)].map(
            |(longitude, latitude, zoom)| Camera {
                longitude,
                latitude,
                zoom,
                roll: 0.,
            },
        );
        let ngc = Index::new(load_ngc(&root.join("ngc.dat")).unwrap());
        for (cam, wanted) in cameras
            .into_iter()
            .zip([(10213, 2561), (1972, 346), (375, 81)])
        {
            let mut count = (0, 0);
            ngc.visible(cam, |row, _| {
                count.0 += 1;
                if row.magnitude == 0. || row.magnitude <= 6.5 {
                    count.1 += 1;
                }
            });
            assert_eq!(count, wanted);
        }
        // C++ SkyCatalogIndex::projectとSkyCatalogRenderer.mmの800×480点・4点集計による順序付き画素数ダイジェスト。
        for (kind, file, counts, hashes) in [
            (
                SdssKind::Galaxy,
                "SDSS_GAL.dat",
                [826360, 71091, 24099],
                [
                    7488955316460785008_u64,
                    3625503464506499543,
                    8740752777904827275,
                ],
            ),
            (
                SdssKind::Quasar,
                "SDSS_QSO.dat",
                [104848, 8680, 2985],
                [
                    16408358073907097890_u64,
                    16408389385369071802,
                    11261298374729271017,
                ],
            ),
        ] {
            let rows = load_sdss(&root.join(file), kind).unwrap();
            let index = Index::new(rows.iter().map(|r| [r.ra_deg, r.dec_deg]));
            for ((cam, count), hash) in cameras.into_iter().zip(counts).zip(hashes) {
                let pixels = density(&index, cam, [200, 120]);
                assert_eq!(pixels.iter().sum::<u32>(), count);
                assert_eq!(
                    pixels
                        .into_iter()
                        .fold(0_u64, |v, n| v.wrapping_mul(31).wrapping_add(n.into())),
                    hash
                );
            }
        }
    }
}
