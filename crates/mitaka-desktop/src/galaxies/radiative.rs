//! Mitaka_plus f9fe624 OurGalaxyRadiativeRenderer.mm の銀河座標と視線。
use crate::solar::{precise::AU_PER_PC, SolarCamera};
use eframe::egui::{self, Color32, ColorImage, Painter, Pos2, Rect, TextureHandle};
use glam::DVec3;
use mitaka_core::astronomy::{equatorial_direction, j2000_equatorial_to_ecliptic};
use mitaka_core::galaxy_radiative::RadiativeModel;
use std::sync::OnceLock;
use std::{
    io,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
};

pub(crate) fn center() -> [f64; 3] {
    let gc = equatorial_direction(
        (17. + 45. / 60. + 40.0383 / 3600.) * 15.,
        -(29. + 28.069 / 3600.),
    )
    .unwrap()
    .map(|v| f64::from(v as f32));
    let [x, y, z] = gc;
    let (s, c) = 23.43870583_f64.to_radians().sin_cos();
    [x, c * y + s * z, -s * y + c * z].map(|v| v * 8000.)
}
pub(super) fn components(p: [f64; 3]) -> [f64; 3] {
    static AXES: OnceLock<[DVec3; 3]> = OnceLock::new();
    let axes = AXES.get_or_init(|| {
        [DVec3::X, DVec3::Y, DVec3::Z].map(|v| {
            DVec3::from_array(
                j2000_equatorial_to_ecliptic(crate::layers::grid::galactic_to_j2000(v.to_array()))
                    .unwrap(),
            )
        })
    });
    axes.map(|axis| axis.dot(DVec3::from_array(p)))
}
pub(super) fn galactic_view(c: SolarCamera) -> Option<([f32; 3], f32)> {
    let anchor = c.anchor();
    let out = c.basis()[2];
    let eye = std::array::from_fn(|i| {
        anchor.anchor_pc[i] + anchor.offset_au[i] / AU_PER_PC + c.distance / AU_PER_PC * out[i]
    });
    let mut origin = components(eye);
    origin[0] -= 8000.;
    if origin.iter().any(|v| v.abs() > 1e7) {
        return None;
    }
    let fade = |r: f32| ((r - 1000.) / 1700.).clamp(0., 1.);
    let alpha = fade(DVec3::from_array(eye).length() as f32)
        * fade(DVec3::from_array(origin).length() as f32);
    (alpha >= 0.02).then_some((origin.map(|v| v as f32), alpha))
}
#[derive(Clone, Copy, PartialEq)]
struct View {
    camera: SolarCamera,
    size: [usize; 2],
    aspect: f64,
}
impl View {
    fn image(self, model: &RadiativeModel, cancel: &AtomicBool) -> io::Result<Option<ColorImage>> {
        let [w, h] = self.size;
        let [right, up, outward] = self.camera.basis().map(components);
        let Some((eye, alpha)) = galactic_view(self.camera) else {
            return Ok(None);
        };
        let bg = [0.016, 0.024, 0.043];
        let tangent = (self.camera.fov.to_radians() / 2.).tan();
        let mut image = ColorImage::new([w, h], Color32::BLACK);
        for y in 0..h {
            if cancel.load(Ordering::Relaxed) {
                return Ok(None);
            }
            for x in 0..w {
                let vx = (2. * (x as f64 + 0.5) - w as f64) / w as f64 * self.aspect * tangent;
                let vy = (h as f64 - 2. * (y as f64 + 0.5)) / h as f64 * tangent;
                let rgb = if alpha >= 0.02 {
                    let direction = std::array::from_fn(|axis| {
                        (-outward[axis] + vx * right[axis] + vy * up[axis]) as f32
                    });
                    model.trace(eye, direction, 40.)?.compose(bg, alpha)?
                } else {
                    bg
                };
                let [r, g, b] = rgb.map(|channel| (channel.clamp(0., 1.) * 255.).round() as u8);
                image.pixels[y * w + x] = Color32::from_rgb(r, g, b);
            }
        }
        Ok(Some(image))
    }
}
struct Pending {
    view: View,
    cancel: Arc<AtomicBool>,
    receiver: mpsc::Receiver<io::Result<Option<ColorImage>>>,
}
impl Drop for Pending {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}
#[derive(Default)]
pub(super) struct Radiation {
    pending: Option<Pending>,
    texture: Option<TextureHandle>,
    finished: Option<View>,
    error: Option<String>,
}
impl Radiation {
    pub(super) fn ready(&self, camera: SolarCamera) -> io::Result<bool> {
        if let Some(error) = &self.error {
            return Err(io::Error::other(error.clone()));
        }
        Ok(self.pending.is_none() && self.finished.is_some_and(|v| v.camera == camera))
    }
    pub(super) fn draw(
        &mut self,
        painter: &Painter,
        model: &Arc<RadiativeModel>,
        rect: Rect,
        camera: SolarCamera,
    ) {
        let scale = (320. / rect.width().max(rect.height()).max(1.)).min(1.);
        let size =
            [rect.width(), rect.height()].map(|v| (v * scale).ceil().clamp(1., 320.) as usize);
        let view = View {
            camera,
            size,
            aspect: f64::from(rect.width()) / f64::from(rect.height().max(1.)),
        };
        if let Some(pending) = &self.pending {
            if pending.view != view {
                pending.cancel.store(true, Ordering::Relaxed);
            }
            let completed = match pending.receiver.try_recv() {
                Ok(image) => Some(image),
                Err(mpsc::TryRecvError::Disconnected) => {
                    Some(Err(io::Error::other("銀河放射モデルの描画が終了しました")))
                }
                Err(mpsc::TryRecvError::Empty) => None,
            };
            if let Some(result) = completed {
                if pending.view == view {
                    match result {
                        Ok(Some(image)) => {
                            self.texture = Some(painter.ctx().load_texture(
                                "銀河の放射・吸収",
                                image,
                                egui::TextureOptions::LINEAR,
                            ));
                            self.finished = Some(view);
                            self.error = None;
                        }
                        Err(e) => {
                            self.error = Some(e.to_string());
                            self.finished = Some(view);
                            self.texture = None;
                        }
                        Ok(None) => {}
                    }
                }
                self.pending = None;
            }
        }
        if self.pending.is_none() && self.finished != Some(view) {
            let (send, receiver) = mpsc::sync_channel(1);
            let ctx = painter.ctx().clone();
            let model = model.clone();
            let cancel = Arc::new(AtomicBool::new(false));
            let flag = cancel.clone();
            self.pending = Some(Pending {
                view,
                receiver,
                cancel,
            });
            std::thread::spawn(move || {
                let output = view.image(&model, &flag);
                let _ = send.send(output);
                ctx.request_repaint();
            });
        }
        if let Some(t) = self
            .texture
            .as_ref()
            .filter(|_| self.finished == Some(view))
        {
            painter.image(
                t.id(),
                rect,
                Rect::from_min_max(Pos2::ZERO, egui::pos2(1., 1.)),
                Color32::WHITE,
            );
        } else if let Some(error) = &self.error {
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                error,
                egui::FontId::proportional(14.),
                Color32::LIGHT_GRAY,
            );
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use mitaka_core::galaxy::OurGalaxyModel;
    #[test]
    #[ignore = "requires Mitaka Plus data"]
    fn dust_map_image_is_nonuniform_and_orientation_dependent() {
        let path = std::path::PathBuf::from(std::env::var_os("MITAKA_DATA_ROOT").unwrap())
            .join("ourgalaxy_model.dat");
        let m = RadiativeModel::from_model(&OurGalaxyModel::load(&path).unwrap()).unwrap();
        let mut camera = SolarCamera::default();
        camera.set_target_pc(center());
        camera.distance = 40000. * AU_PER_PC;
        let mut view = View {
            camera,
            size: [64, 40],
            aspect: 1.6,
        };
        let flag = AtomicBool::new(false);
        let a = view.image(&m, &flag).unwrap().unwrap();
        assert!(a.pixels.iter().any(|v| v.r() > 60));
        view.camera.yaw += 27.;
        let b = view.image(&m, &flag).unwrap().unwrap();
        assert_ne!(a.pixels, b.pixels);
        flag.store(true, Ordering::Relaxed);
        assert!(view.image(&m, &flag).unwrap().is_none());
    }
}
