//! 元の SolarSceneRenderer.mm と同じ順で背後の軌跡を描き、手前を実表面深度で判定する。
use crate::solar::{clip_line, sphere::Surfaces, SolarCamera};
use eframe::egui::{self, Color32, Pos2, Rect, Shape, Stroke};
#[derive(Default)]
pub(super) struct DrawList {
    pub back: Vec<Shape>,
    pub front: Vec<Shape>,
}
pub(super) struct Canvas<'a> {
    pub painter: &'a egui::Painter,
    pub camera: SolarCamera,
    pub rect: Rect,
    pub surface: &'a Surfaces,
    pub out: &'a mut DrawList,
}
impl Canvas<'_> {
    fn length(&self, p: Pos2, depth: f64) -> f64 {
        let focal =
            f64::from(self.rect.height()) / (2. * (self.camera.fov * 0.5).to_radians().tan());
        let d = p - self.rect.center();
        depth * ((f64::from(d.x) / focal).hypot(f64::from(d.y) / focal)).hypot(1.)
    }
    pub fn visible(&self, p: Pos2, depth: f64) -> bool {
        self.length(p, depth) < self.surface.depth_at(p)
    }
    pub fn push(&mut self, p: Pos2, depth: f64, shape: Shape) {
        // ラベルが輪郭をまたぐ場合、後方の部分も惑星面で自然に切り抜かれる。
        if self.surface.depth_at(p).is_finite() && self.visible(p, depth) {
            self.out.front.push(shape);
        } else {
            self.out.back.push(shape);
        }
    }
    pub fn path(&mut self, samples: impl IntoIterator<Item = Option<[f64; 3]>>, color: Color32) {
        let stroke = Stroke::new(0.8_f32, color);
        let mut previous = None;
        for q in samples {
            if let Some((a, b)) = previous.zip(q) {
                if let Some([a, b]) = self.camera.project_segment(a, b, self.rect) {
                    self.segment(a, b, stroke);
                }
            }
            previous = q;
        }
    }
    fn segment(&mut self, a: (Pos2, f64), b: (Pos2, f64), stroke: Stroke) {
        let Some([start, end]) = clip_line(self.rect, a.0, b.0) else {
            return;
        };
        self.out
            .back
            .push(Shape::line_segment([start, end], stroke));
        if !self
            .surface
            .intersects_depth(Rect::from_two_pos(start, end).expand(1.))
        {
            return;
        }
        let delta = b.0 - a.0;
        let kd = glam::DVec2::new(delta.x.into(), delta.y.into());
        if kd.length_squared() == 0. {
            return;
        }
        let depth = |p: Pos2| {
            let v = p - a.0;
            let t = (glam::DVec2::new(v.x.into(), v.y.into()).dot(kd) / kd.length_squared())
                .clamp(0., 1.);
            ((1. - t) / a.1 + t / b.1).recip()
        };
        let mut front = Vec::new();
        let count = start.distance(end).ceil().max(1.) as usize;
        front_spans(
            start,
            end,
            count,
            |p| {
                let hit = self.surface.depth_at(p);
                hit.is_finite() && self.length(p, depth(p)) < hit
            },
            |seg| front.push(Shape::line_segment(seg, stroke)),
        );
        self.out.front.extend(front);
    }
}
fn front_spans(
    start: Pos2,
    end: Pos2,
    count: usize,
    mut front: impl FnMut(Pos2) -> bool,
    mut draw: impl FnMut([Pos2; 2]),
) {
    let delta = end - start;
    let mut began = None;
    for i in 0..count {
        let p = start + delta * ((i as f32 + 0.5) / count as f32);
        if front(p) {
            began.get_or_insert_with(|| start + delta * (i as f32 / count as f32));
        } else if let Some(s) = began.take() {
            draw([s, start + delta * (i as f32 / count as f32)]);
        }
    }
    if let Some(s) = began {
        draw([s, end]);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn path_remains_visible_only_across_foreground_surface_samples() {
        let mut spans = vec![];
        front_spans(
            Pos2::new(0., 0.),
            Pos2::new(12., 0.),
            12,
            |p| p.x > 3. && p.x < 8.,
            |p| spans.push(p),
        );
        assert_eq!(spans, vec![[Pos2::new(3., 0.), Pos2::new(8., 0.)]]);
    }
}

#[cfg(test)]
mod physical_geometry {
    use super::*;
    #[test]
    fn foreground_objects_and_hidden_clicks_share_the_same_disk_and_ray_distance() {
        let ctx = egui::Context::default();
        let mut surface = Surfaces::new();
        let p = Pos2::new(100., 60.);
        surface.symbol(p, 8., 1.);
        let _ = ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let mut out = DrawList::default();
                let rect = Rect::from_center_size(p, egui::vec2(200., 120.));
                let mut canvas = Canvas {
                    painter: ui.painter(),
                    surface: &surface,
                    rect,
                    camera: SolarCamera::default(),
                    out: &mut out,
                };
                assert!(!canvas.visible(p, 2.));
                assert!(canvas.visible(p, 0.5));
                assert!(canvas.visible(p + egui::vec2(9., 0.), 2.));
                canvas.push(p, 2., Shape::Noop);
                canvas.push(p, 0.5, Shape::Noop);
                assert_eq!((out.back.len(), out.front.len()), (1, 1));
            });
        });
    }
    #[test]
    fn surface_camera_clips_crossing_paths_and_nearby_objects_at_native_near_plane() {
        let mut c = SolarCamera {
            distance: 1.,
            ..Default::default()
        };
        c.frame = Some([[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]]);
        let rect = Rect::from_min_max(Pos2::ZERO, Pos2::new(640., 480.));
        assert!(c.project([0., 0., 1. - 1e-8], rect).is_some());
        let [a, b] = c
            .project_segment([0., 0., 1.000001], [0., 0., 1. - 1e-8], rect)
            .unwrap();
        assert_eq!(a.0, rect.center());
        assert_eq!(a.1, 1e-12);
        assert!(b.1 > a.1);
        c.frame_is_orbit = true;
        assert!(c
            .project_segment([0., 0., 1.000001], [0., 0., 1. - 1e-8], rect)
            .is_none());
    }
}
