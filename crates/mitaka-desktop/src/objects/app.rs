use super::Position;
use crate::{chart::Chart, projection::View, App};
use eframe::egui::{self, Align2, Color32, FontId, Pos2, Stroke};
use mitaka_core::astronomy::ObservationFrame;

fn marker(painter: &egui::Painter, p: Pos2, name: &str) {
    if painter.clip_rect().contains(p) {
        let c = Color32::LIGHT_GREEN;
        painter.circle_stroke(p, 7., Stroke::new(1.5_f32, c));
        painter.text(
            p + egui::vec2(10., 8.),
            Align2::LEFT_TOP,
            name,
            FontId::proportional(12.),
            c,
        );
    }
}
impl App {
    pub(crate) fn object_panels(&mut self, ctx: &egui::Context) {
        let Some(data) = self.data.as_ref() else {
            return;
        };
        let Some(target) = self
            .objects
            .panel(ctx, data, [self.jd, self.latitude, self.longitude])
        else {
            return;
        };
        match target {
            Position::Sky(coords, hip) => {
                self.selected = hip;
                match self.objects.kind {
                    super::Kind::Ngc => {
                        self.options.layers.deep_sky.ngc = true;
                        if let Some(m) = self
                            .objects
                            .data
                            .as_ref()
                            .and_then(|c| c.nebula_magnitude(self.objects.active?))
                        {
                            self.options.magnitude = self.options.magnitude.max(m);
                        }
                    }
                    super::Kind::Galaxies => self.options.layers.deep_sky.galaxies = true,
                    super::Kind::Quasars => self.options.layers.deep_sky.quasars = true,
                    _ => {}
                }
                if let Some(s) = hip
                    .and_then(|h| data.by_hip.get(&h))
                    .and_then(|&i| data.stars.get(i))
                {
                    self.options.magnitude = self.options.magnitude.max(s.magnitude);
                }
                let (view, coords) = if self.view == View::Horizon {
                    match ObservationFrame::new(self.latitude, self.longitude, self.jd)
                        .and_then(|f| f.to_horizontal(coords[0], coords[1]))
                    {
                        Ok(horizontal) => (View::Horizon, horizontal),
                        Err(e) => {
                            self.error = Some(e.to_string());
                            return;
                        }
                    }
                } else {
                    (View::Sky, coords)
                };
                self.view = view;
                self.camera().center(view, coords);
            }
            Position::Solar(point, _)
                if self.view == View::Horizon
                    && matches!(
                        self.objects.kind,
                        super::Kind::Small | super::Kind::Spacecraft
                    ) =>
            {
                let observed = mitaka_core::topocentric::Observer::from_ephemeris(
                    &data.ephemeris,
                    self.jd,
                    self.latitude,
                    self.longitude,
                )
                .and_then(|site| site.observe(point));
                match observed {
                    Ok(apparent) => {
                        self.objects
                            .horizon
                            .focus(self.objects.kind, self.objects.active);
                        self.horizon.center(View::Horizon, apparent.horizontal_deg);
                    }
                    Err(e) => self.error = Some(e.to_string()),
                }
            }
            Position::Solar(point, distance) => {
                let previous_view = self.view;
                self.view = View::Solar;
                self.solar.stop();
                self.solar.camera.target = point;
                if previous_view != View::Solar || self.solar.camera.distance > distance * 10. {
                    self.solar.camera.distance = distance;
                }
            }
        }
    }
    pub(crate) fn object_on_sky(&self, chart: &Chart<'_>, painter: &egui::Painter) {
        if let Some((label, Position::Sky([ra, dec], _))) = &self.objects.marker {
            if let Some(p) = chart.observed(*ra, *dec) {
                marker(painter, chart.point(p), label);
            }
        }
    }
    pub(crate) fn object_in_solar(&mut self, ui: &egui::Ui) {
        let Some(sky) = &self.data else { return };
        let rect = self.canvas_rect;
        let click = self.picking.raw_pointer(ui, rect);
        let mut hit = None;
        let mut nearest = f32::INFINITY;
        for kind in [
            super::Kind::Small,
            super::Kind::Spacecraft,
            super::Kind::Satellites,
        ] {
            if !self.objects.display.group_visible(kind) {
                continue;
            }
            let Some(c) = self.objects.scene_catalog(kind, &sky.root, ui.ctx()) else {
                continue;
            };
            let scene = super::layers::Overlay {
                catalog: &c,
                kind,
                active: self.objects.active.filter(|_| self.objects.kind == kind),
                display: &self.objects.display,
                selections: &self.objects.selections,
                sky,
                jd: self.jd,
            };
            let paths = scene.permits_paths();
            if !paths {
                ui.painter().text(rect.left_top(),egui::Align2::LEFT_TOP,"軌道の標本数が上限を超えました。対象の絞り込み、または標本数の変更が必要です。",egui::FontId::proportional(12.),Color32::YELLOW);
            }
            if let Some((i, d)) = scene.draw(ui.painter(), self.solar.camera, rect, click, paths) {
                if d < nearest {
                    nearest = d;
                    hit = Some((kind, i, c));
                }
            }
        }
        if let Some((kind, i, c)) = hit {
            self.objects
                .select_scene(kind, i, c, self.picking == crate::viewer::Picking::Click);
        }
    }
}
