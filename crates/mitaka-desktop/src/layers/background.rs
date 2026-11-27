//! Mitaka_plus f9fe624 の sky_background.mm と grid_geometry.cpp に基づく天の川背景。
pub(crate) mod solar;
use super::grid::{galactic_from_j2000, Vector};
use crate::{
    chart::Chart,
    media_preview::image::load_landscape,
    projection::{Camera, View},
};
use eframe::egui::{self, Color32, ColorImage, Painter, Rect, TextureHandle};
use image::RgbaImage;
use mitaka_core::astronomy::{equatorial_direction, ObservationFrame};
use std::{
    fs, io,
    path::Path,
    sync::{mpsc, Arc},
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u8)]
pub enum Mode {
    #[default]
    None,
    Normal,
    Bright,
    Infrared,
}
impl Mode {
    pub const ALL: [Self; 4] = [Self::None, Self::Normal, Self::Bright, Self::Infrared];
    pub fn name(self) -> &'static str {
        ["None", "Normal", "Bright", "Infrared"][self as usize]
    }
    pub fn label(self) -> &'static str {
        ["なし", "標準", "明るい", "赤外線"][self as usize]
    }
}
#[derive(Default)]
pub struct Backgrounds {
    sources: [Option<Arc<RgbaImage>>; 3],
}
impl Backgrounds {
    pub fn load(data: &Path, warnings: &mut Vec<String>) -> Self {
        let dir = data.parent().unwrap_or(data).join("textures");
        let sources = ["milkyway.jpg", "milkyway2.jpg", "MilkyWay_IR.jpg"].map(|name| {
            let mut path = dir.join(name);
            if name == "MilkyWay_IR.jpg"
                && fs::symlink_metadata(&path).is_err_and(|e| e.kind() == io::ErrorKind::NotFound)
            {
                path = dir.join("milkyway_IR.jpg");
            }
            match fs::symlink_metadata(&path).and_then(|meta| {
                if !meta.is_file() {
                    return Err(io::Error::other(
                        "天の川画像は通常ファイルで指定してください",
                    ));
                }
                load_landscape(&path)
            }) {
                Ok(image) => Some(Arc::new(image)),
                Err(error) => {
                    warnings.push(format!("天の川画像 {}: {error}", path.display()));
                    None
                }
            }
        });
        Self { sources }
    }
    fn get(&self, mode: Mode) -> Option<&Arc<RgbaImage>> {
        match mode {
            Mode::None => None,
            _ => self.sources[mode as usize - 1].as_ref(),
        }
    }
}
fn uv([x, y, z]: Vector) -> [f64; 2] {
    let xy = x.hypot(y);
    let lon = if xy < 1e-14 { 0. } else { y.atan2(x) };
    [
        (0.5 - lon / std::f64::consts::TAU).rem_euclid(1.),
        (0.5 - z.atan2(xy) / std::f64::consts::PI).clamp(0., 1.),
    ]
}
/// 水平は周期境界、極は端行を保持する。透明画素の色を隣へ漏らさず補間する。
fn sample(image: &RgbaImage, [u, v]: [f64; 2]) -> [u8; 4] {
    let (w, h) = image.dimensions();
    let x = u * f64::from(w) - 0.5;
    let y = (v * f64::from(h) - 0.5).clamp(0., f64::from(h - 1));
    let left = (x.floor() as i64).rem_euclid(i64::from(w)) as u32;
    let right = (left + 1) % w;
    let top = y as u32;
    let bottom = (top + 1).min(h - 1);
    let dx = x - x.floor();
    let dy = y - f64::from(top);
    let mut premultiplied = [0.; 3];
    let mut alpha = 0.;
    for (col, row, weight) in [
        (left, top, (1. - dx) * (1. - dy)),
        (right, top, dx * (1. - dy)),
        (left, bottom, (1. - dx) * dy),
        (right, bottom, dx * dy),
    ] {
        let p = image.get_pixel(col, row);
        let a = f64::from(p[3]) * weight;
        alpha += a;
        for ch in 0..3 {
            premultiplied[ch] += f64::from(p[ch]) * a;
        }
    }
    if alpha <= 0. {
        return [0; 4];
    }
    let [r, g, b] = premultiplied.map(|v| (v / alpha).round() as u8);
    [r, g, b, alpha.round() as u8]
}
#[derive(Clone, Copy)]
struct Raster {
    columns: [Vector; 3],
    horizontal_deg: f64,
    horizontal_span: f64,
    vertical_deg: f64,
    vertical_span: f64,
}
impl Raster {
    fn new(view: View, camera: Camera, frame: &ObservationFrame) -> io::Result<Self> {
        let span = 360. / camera.zoom;
        if view == View::Sky {
            return Ok(Self {
                columns: [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]].map(galactic_from_j2000),
                horizontal_deg: camera.longitude,
                horizontal_span: -span,
                vertical_deg: camera.latitude,
                vertical_span: span / 2.,
            });
        }
        let direction = |az, alt| -> io::Result<Vector> {
            let [ra, dec] = frame.to_equatorial(az, alt)?;
            Ok(galactic_from_j2000(equatorial_direction(ra, dec)?))
        };
        let forward = direction(camera.longitude, camera.latitude)?;
        let right = direction(camera.longitude + 90., 0.)?;
        let up = direction(
            camera.longitude + if camera.latitude > 0. { 180. } else { 0. },
            90. - camera.latitude.abs(),
        )?;
        // 原版は 360−方位角を赤経として回転する。東を正とする地平系ではロールの符号が反転する。
        let (sr, cr) = camera.roll.to_radians().sin_cos();
        let columns = [
            forward,
            std::array::from_fn(|i| cr * right[i] + sr * up[i]),
            std::array::from_fn(|i| cr * up[i] - sr * right[i]),
        ];
        Ok(Self {
            columns,
            horizontal_deg: 0.,
            horizontal_span: span,
            vertical_deg: 0.,
            vertical_span: span / 2.,
        })
    }
    fn rows(self, size: [usize; 2], mut pixel: impl FnMut([f64; 2])) {
        let [w, h] = size;
        let horizontal: Vec<Vector> = (0..w)
            .map(|x| {
                let angle = self.horizontal_deg
                    + ((x as f64 + 0.5) / w as f64 - 0.5) * self.horizontal_span;
                let (s, c) = angle.to_radians().sin_cos();
                std::array::from_fn(|i| c * self.columns[0][i] + s * self.columns[1][i])
            })
            .collect();
        for y in 0..h {
            let angle =
                self.vertical_deg + (0.5 - (y as f64 + 0.5) / h as f64) * self.vertical_span;
            let (s, c) = angle.to_radians().sin_cos();
            let z = self.columns[2].map(|v| s * v);
            for base in &horizontal {
                pixel(uv(std::array::from_fn(|i| c * base[i] + z[i])));
            }
        }
    }
    fn render(self, image: &RgbaImage, size: [usize; 2]) -> ColorImage {
        let mut pixels = Vec::with_capacity(size[0] * size[1]);
        self.rows(size, |at| {
            let [r, g, b, a] = sample(image, at);
            // egui の加算色は alpha=0、RGBは既にアルファ適用済み。原版の PlusLighter に対応する。
            let [r, g, b] = [r, g, b].map(|v| ((u16::from(v) * u16::from(a) + 127) / 255) as u8);
            pixels.push(Color32::from_rgba_premultiplied(r, g, b, 0));
        });
        ColorImage { size, pixels }
    }
}
#[derive(Clone)]
struct Key {
    image: Arc<RgbaImage>,
    view: View,
    camera: [f64; 4],
    site: [f64; 3],
    size: [usize; 2],
}
impl Key {
    fn same_view(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.image, &other.image)
            && self.view == other.view
            && self.camera == other.camera
            && self.site[..2] == other.site[..2]
            && self.size == other.size
    }
}
#[derive(Default)]
pub struct Renderer {
    job: Option<mpsc::Receiver<(Key, ColorImage)>>,
    cached: Option<(Key, TextureHandle)>,
}
impl Renderer {
    pub fn paint(
        &mut self,
        painter: &Painter,
        chart: &Chart<'_>,
        data: &Backgrounds,
        mode: Mode,
        site: [f64; 3],
    ) {
        if let Some(job) = &self.job {
            match job.try_recv() {
                Ok((key, image)) => {
                    self.cached = Some((
                        key,
                        painter
                            .ctx()
                            .load_texture("天の川", image, egui::TextureOptions::LINEAR),
                    ));
                    self.job = None;
                }
                Err(mpsc::TryRecvError::Disconnected) => self.job = None,
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        let Some(image) = data.get(mode) else { return };
        let rect = chart.rect;
        let scale = (1024. / rect.width().max(rect.height())).min(1.);
        let size =
            [rect.width(), rect.height()].map(|v| (v * scale).ceil().clamp(1., 1024.) as usize);
        let camera = chart.camera;
        let key = Key {
            image: image.clone(),
            size,
            view: chart.view,
            camera: [camera.longitude, camera.latitude, camera.zoom, camera.roll],
            site: if chart.view == View::Sky {
                [0.; 3]
            } else {
                site
            },
        };
        if let Some((cached, texture)) = &self.cached {
            if cached.same_view(&key) {
                if let Some(f) = chart.flight {
                    f.image(painter, texture.id(), rect);
                } else {
                    painter.image(
                        texture.id(),
                        rect,
                        Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1., 1.)),
                        Color32::WHITE,
                    );
                }
                if cached.site[2] == key.site[2] {
                    return;
                }
            }
        }
        if self.job.is_none() {
            let Ok(raster) = Raster::new(chart.view, camera, chart.frame) else {
                return;
            };
            let (tx, rx) = mpsc::sync_channel(1);
            let ctx = painter.ctx().clone();
            self.job = Some(rx);
            std::thread::spawn(move || {
                let pixels = raster.render(&key.image, size);
                if tx.send((key, pixels)).is_ok() {
                    ctx.request_repaint();
                }
            });
        }
    }
}

#[cfg(test)]
mod tests;
