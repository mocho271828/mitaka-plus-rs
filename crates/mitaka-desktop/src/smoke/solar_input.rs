//! 太陽系キャンバスへ egui の入力イベントを渡し、描画後の視点を確認する。
use crate::{
    catalog::SkyData,
    galaxies,
    solar::{self, Solar},
    viewer::Picking,
};
use eframe::egui::{self, Context, Event, Rect};
use std::io;

struct Canvas<'a> {
    data: &'a SkyData,
    solar: Solar,
    ctx: Context,
    galaxies: galaxies::Browser,
}
impl<'a> Canvas<'a> {
    fn new(data: &'a SkyData) -> Self {
        let ctx = Context::default();
        Self {
            data,
            solar: Solar::default(),
            galaxies: galaxies::Browser::new(data.root.clone(), &ctx),
            ctx,
        }
    }
    fn frame(&mut self, jd: f64, events: Vec<Event>) -> io::Result<()> {
        let rect = Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(480., 320.));
        let mut result = Ok(());
        let _ = self.ctx.run(
            egui::RawInput {
                events,
                screen_rect: Some(rect),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    result = self.solar.draw(
                        ui,
                        solar::ViewSettings {
                            data: self.data,
                            jd,
                            picking: Picking::None,
                            layers: &Default::default(),
                            galaxies: &mut self.galaxies,
                            surface_site: None,
                            observer: [35., 139.],
                            star_display: solar::sky::Display {
                                magnitude: 0.,
                                lines: false,
                                labels: false,
                                selected: None,
                            },
                            horizon_camera: Default::default(),
                            terrain_mode: Default::default(),
                        },
                    );
                });
            },
        );
        result
    }
}
pub(super) fn check_tracking(data: &SkyData) -> io::Result<()> {
    let mut canvas = Canvas::new(data);
    let jd = data.initial_date(crate::J2000);
    // 合成暦表の火星は固定位置なので、同じ入力経路を移動する月でも検査する。
    for body in [4, 10] {
        canvas.frame(jd, vec![Event::PointerMoved(egui::pos2(210., 157.))])?;
        canvas.solar.approach(body)?;
        let initial = canvas.solar.camera.distance;
        canvas.frame(jd, vec![Event::Zoom(0.5)])?;
        let distance = canvas.solar.camera.distance;
        canvas.frame((jd + 1.).min(data.date_range[1]), vec![])?;
        if (distance - 2. * initial).abs() > 1e-12
            || canvas.solar.camera.distance != distance
            || Some(canvas.solar.camera.target) != canvas.solar.body_point(body)
        {
            return Err(io::Error::other(format!(
                "ピンチ後に {} の現在位置と距離を維持できません",
                solar::NAMES[body]
            )));
        }
    }
    Ok(())
}
