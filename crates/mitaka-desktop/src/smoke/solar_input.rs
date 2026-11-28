//! 太陽系キャンバスへ egui の入力イベントを渡し、描画後の視点を確認する。
use crate::{
    camera_control::session::Site,
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
    site: Option<Site>,
    ctx: Context,
    galaxies: galaxies::Browser,
}
impl<'a> Canvas<'a> {
    fn new(data: &'a SkyData) -> Self {
        let ctx = Context::default();
        Self {
            data,
            solar: Solar::default(),
            site: None,
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
                            surface_site: self.site.as_mut(),
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

pub(super) fn check_surface(data: &SkyData) -> io::Result<()> {
    let mut canvas = Canvas::new(data);
    let jd = data.initial_date(crate::J2000);
    let start = egui::pos2(210., 157.);
    canvas.frame(jd, vec![Event::PointerMoved(start)])?;
    let mut site = Site::ground(10, 0., 0., 0., Default::default());
    site.heading = 0.;
    canvas.solar.selected = 10;
    site.pose(
        canvas.solar.body_point(10).unwrap().into(),
        jd,
        &mut canvas.solar.camera,
    )?;
    canvas.site = Some(site);
    let fov = canvas.solar.camera.fov;
    for (event, height) in [
        (Event::Zoom(0.5), 0.004),
        (Event::Zoom(0.5), 0.008),
        (Event::Zoom(8.), 0.002),
    ] {
        canvas.frame(jd, vec![event])?;
        if (canvas.site.unwrap().height - height).abs() > 1e-10 || canvas.solar.camera.fov != fov {
            return Err(io::Error::other("地表ズームの高度・視野角が一致しません"));
        }
    }
    canvas.frame(
        jd,
        vec![Event::PointerButton {
            pos: start,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        }],
    )?;
    for (point, heading) in [(start + egui::vec2(20., 0.), 353.), (start, 0.)] {
        canvas.frame(jd, vec![Event::PointerMoved(point)])?;
        if (canvas.site.unwrap().heading - heading).abs() > 1e-9 {
            return Err(io::Error::other("地表ドラッグの方位が一致しません"));
        }
    }
    Ok(())
}
