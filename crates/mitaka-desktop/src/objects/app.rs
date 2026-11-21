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
            if let Some(p) = chart.observed(*ra as f32, *dec as f32) {
                marker(painter, chart.point(p), label);
            }
        }
    }
    pub(crate) fn object_in_solar(&mut self, ui: &egui::Ui) {
        let (Some(c), Some(sky)) = (&self.objects.data, &self.data) else {
            return;
        };
        let rect = self.canvas_rect;
        let click = self.picking.raw_pointer(ui, rect);
        let scene = super::layers::Overlay {
            catalog: c,
            kind: self.objects.kind,
            active: self.objects.active,
            options: &self.objects.options,
            selections: &self.objects.selections,
            sky,
            jd: self.jd,
        };
        if let Some(i) = scene.draw(ui.painter(), self.solar.camera, rect, click) {
            self.objects.active = Some(i);
            if self.picking == crate::viewer::Picking::Click {
                self.objects.focus_next = true;
                self.objects.open = true;
            }
            ui.ctx().request_repaint();
        }
    }
}
