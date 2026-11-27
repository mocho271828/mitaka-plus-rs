//! f9fe624 sky_background.mm の太陽系視線と SolarSystemView.mm の距離フェード。
use super::{galactic_from_j2000, sample, uv, Backgrounds, Mode};
use crate::solar::{precise::AU_PER_PC, SolarCamera};
use eframe::egui::{self, Color32, ColorImage, Painter, Rect, TextureHandle};
use glam::DVec3;
use image::RgbaImage;
use mitaka_core::astronomy::j2000_ecliptic_to_equatorial;
use std::{io, sync::Arc};

fn opacity(camera: SolarCamera) -> f64 {
    let target = camera.anchor();
    let eye = DVec3::from(target.anchor_pc)
        + DVec3::from(target.offset_au) / AU_PER_PC
        + DVec3::from(camera.basis()[2]) * (camera.distance / AU_PER_PC);
    ((2700. - eye.length()) / 1700.).clamp(0., 1.)
}
fn render(image: &RgbaImage, camera: SolarCamera, [w, h]: [usize; 2]) -> io::Result<ColorImage> {
    // 線形変換をカメラの3基底に一度だけ適用する。銀河基底の非直交成分も保持する。
    let axes = camera.basis().map(|d| {
        j2000_ecliptic_to_equatorial(d)
            .map(galactic_from_j2000)
            .map(DVec3::from)
    });
    let [r, u, o] = axes;
    let (right, up, out) = (r?, u?, o?);
    let step = 2. * (camera.fov.max(1e-9) * 0.5).to_radians().tan() / h as f64;
    let columns: Vec<_> = (0..w)
        .map(|x| right * ((x as f64 + 0.5 - w as f64 * 0.5) * step) - out)
        .collect();
    let mut pixels = Vec::with_capacity(w * h);
    for y in 0..h {
        let v = up * ((h as f64 * 0.5 - y as f64 - 0.5) * step);
        for &c in &columns {
            let [r, g, b, a] = sample(image, uv((c + v).to_array()));
            let [r, g, b] = [r, g, b].map(|x| ((u16::from(x) * u16::from(a) + 127) / 255) as u8);
            pixels.push(Color32::from_rgb_additive(r, g, b));
        }
    }
    Ok(ColorImage {
        size: [w, h],
        pixels,
    })
}
#[derive(PartialEq)]
struct View {
    basis: [[f64; 3]; 3],
    fov: f64,
    size: [usize; 2],
}
#[derive(Default)]
pub(crate) struct Background {
    cached: Option<(View, Arc<RgbaImage>, TextureHandle)>,
}
impl Background {
    pub fn paint(
        &mut self,
        painter: &Painter,
        rect: Rect,
        camera: SolarCamera,
        backgrounds: &Backgrounds,
        mode: Mode,
        preview: bool,
    ) -> io::Result<()> {
        let Some(image) = backgrounds.get(mode) else {
            return Ok(());
        };
        let alpha = opacity(camera);
        if alpha == 0. || !rect.is_positive() {
            return Ok(());
        }
        let limit = if preview { 512_f32 } else { 1024. };
        let zoom = (limit / rect.width().max(rect.height())).min(1.);
        let size =
            [rect.width(), rect.height()].map(|v| (v * zoom).ceil().clamp(1., limit) as usize);
        let key = View {
            size,
            fov: camera.fov,
            basis: camera
                .basis()
                .map(|axis| axis.map(|c| (c * 1e10).round() / 1e10)),
        };
        if self
            .cached
            .as_ref()
            .is_none_or(|(old, img, _)| old != &key || !Arc::ptr_eq(img, image))
        {
            let texture = painter.ctx().load_texture(
                "太陽系の天の川背景",
                render(image, camera, size)?,
                egui::TextureOptions::LINEAR,
            );
            self.cached = Some((key, image.clone(), texture));
        }
        if let Some((_, _, tex)) = &self.cached {
            painter.image(
                tex.id(),
                rect,
                Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1., 1.)),
                Color32::from_white_alpha((alpha * 255.).round() as u8),
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn samples(image: &RgbaImage) -> [[u8; 3]; 9] {
        let c = SolarCamera {
            yaw: 81.,
            pitch: -27.,
            roll: 36.,
            fov: 71.,
            ..Default::default()
        };
        let output = render(image, c, [9, 5]).unwrap();
        [0, 4, 8, 18, 22, 26, 36, 40, 44].map(|p| {
            let [r, g, b, a] = output.pixels[p].to_array();
            assert_eq!(a, 0);
            [r, g, b]
        })
    }
    #[test]
    fn rotated_camera_rays_and_galactic_axes_match_the_native_raster() {
        let image = RgbaImage::from_fn(64, 32, |x, y| {
            image::Rgba([(4 * x) as u8, (8 * y) as u8, 0, 255])
        });
        assert_eq!(
            samples(&image),
            [
                [98, 167, 0],
                [86, 107, 0],
                [69, 50, 0],
                [114, 163, 0],
                [107, 95, 0],
                [90, 29, 0],
                [130, 154, 0],
                [130, 91, 0],
                [130, 27, 0]
            ]
        );
    }
    #[test]
    fn distance_fade_uses_the_eye_and_precise_target_offset() {
        let mut c = SolarCamera {
            frame: Some([[0., 1., 0.], [0., 0., 1.], [1., 0., 0.]]),
            frame_is_orbit: true,
            distance: AU_PER_PC,
            ..Default::default()
        };
        for (eye, alpha) in [(0., 1.), (1000., 1.), (1850., 0.5), (2700., 0.), (1e40, 0.)] {
            c.set_anchor(crate::solar::precise::Target {
                anchor_pc: [eye - 1., 0., 0.],
                offset_au: [0.; 3],
            });
            assert_eq!(opacity(c), alpha);
        }
        c.set_anchor(crate::solar::precise::Target {
            anchor_pc: [1849., 0., 0.],
            offset_au: [AU_PER_PC, 0., 0.],
        });
        assert!((opacity(c) - 849. / 1700.).abs() < 1e-12);
    }
    #[test]
    #[ignore = "local Mitaka Plus 1.5.4 sky JPEGs"]
    fn native_solar_jpeg_rgb_for_all_three_backgrounds() {
        let root = std::path::PathBuf::from(std::env::var_os("MITAKA_DATA_ROOT").unwrap());
        let mut errors = vec![];
        let src = Backgrounds::load(&root, &mut errors);
        assert!(errors.is_empty(), "{errors:?}");
        let expected = [
            [
                [0, 0, 0],
                [2, 2, 2],
                [0, 0, 0],
                [0, 0, 0],
                [0, 0, 0],
                [0, 0, 0],
                [0, 0, 0],
                [12, 11, 16],
                [0, 0, 0],
            ],
            [
                [0, 0, 10],
                [0, 2, 24],
                [0, 0, 8],
                [0, 1, 6],
                [1, 0, 5],
                [1, 0, 18],
                [0, 0, 0],
                [23, 22, 48],
                [0, 0, 8],
            ],
            [
                [44, 51, 93],
                [85, 106, 129],
                [26, 25, 43],
                [50, 62, 96],
                [62, 78, 112],
                [29, 26, 28],
                [65, 90, 120],
                [64, 64, 84],
                [32, 16, 30],
            ],
        ];
        for (mode, reference) in [Mode::Normal, Mode::Bright, Mode::Infrared]
            .into_iter()
            .zip(expected)
        {
            for (got, want) in samples(src.get(mode).unwrap()).into_iter().zip(reference) {
                assert!(
                    got.into_iter().zip(want).all(|(a, b)| a.abs_diff(b) <= 3),
                    "{mode:?} {got:?} {want:?}"
                );
            }
        }
    }
}
