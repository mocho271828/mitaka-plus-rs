//! 公式1.3.1の銀河中心座標(pc)で光線を積分する。原点・基底が未確認の点群とは別の視点。
//! Mitaka_plus f9fe624 OurGalaxyRadiativeRenderer.mm の透視投影・距離によるフェードを参照。
use eframe::egui::{self, Color32, ColorImage, Painter, Pos2, Rect, TextureHandle};
use mitaka_core::galaxy_radiative::RadiativeModel;
use std::{
    io,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
};

#[derive(Clone, Copy, PartialEq)]
struct View {
    yaw: f64,
    pitch: f64,
    distance: f64,
    size: [usize; 2],
}
impl View {
    fn image(self, model: &RadiativeModel, cancel: &AtomicBool) -> io::Result<Option<ColorImage>> {
        let [w, h] = self.size;
        let (sy, cy) = self.yaw.sin_cos();
        let (sp, cp) = self.pitch.sin_cos();
        let right = [cy, sy, 0.];
        let up = [-sy * sp, cy * sp, cp];
        let outward = [sy * cp, -cy * cp, sp];
        let eye = outward.map(|v| (v * self.distance) as f32);
        let sun = (eye[0] + 8000.).hypot(eye[1]).hypot(eye[2]);
        let fade = |dist: f32| ((dist - 1000.) / 1700.).clamp(0., 1.);
        let alpha = fade(sun) * fade(self.distance as f32);
        let bg = [0.016, 0.024, 0.043];
        let tangent = (45_f64.to_radians() / 2.).tan();
        let mut image = ColorImage::new([w, h], Color32::BLACK);
        for y in 0..h {
            if cancel.load(Ordering::Relaxed) {
                return Ok(None);
            }
            for x in 0..w {
                let vx = (2. * (x as f64 + 0.5) - w as f64) / h as f64 * tangent;
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
    pub(super) fn draw(
        &mut self,
        painter: &Painter,
        model: &Arc<RadiativeModel>,
        rect: Rect,
        yaw: f64,
        pitch: f64,
        zoom: f64,
    ) {
        let scale = (320. / rect.width().max(rect.height()).max(1.)).min(1.);
        let size =
            [rect.width(), rect.height()].map(|v| (v * scale).ceil().clamp(1., 320.) as usize);
        let view = View {
            yaw,
            pitch,
            distance: (40000. / zoom).clamp(1., 1e7),
            size,
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
        if let Some(t) = &self.texture {
            painter.image(
                t.id(),
                rect,
                Rect::from_min_max(Pos2::ZERO, egui::pos2(1., 1.)),
                Color32::WHITE,
            );
        } else {
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                self.error.as_deref().unwrap_or("銀河の放射・吸収を計算中…"),
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
        let mut view = View {
            yaw: 0.55,
            pitch: 0.65,
            distance: 40000.,
            size: [64, 40],
        };
        let flag = AtomicBool::new(false);
        let a = view.image(&m, &flag).unwrap().unwrap();
        assert!(a.pixels.iter().any(|v| v.r() > 60));
        view.yaw += 0.5;
        let b = view.image(&m, &flag).unwrap().unwrap();
        assert_ne!(a.pixels, b.pixels);
        flag.store(true, Ordering::Relaxed);
        assert!(view.image(&m, &flag).unwrap().is_none());
    }
}
