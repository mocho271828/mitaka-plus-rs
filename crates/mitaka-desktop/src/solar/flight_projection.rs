//! sky_model.cpp の地平投影。blend は画素から復元する角度へ適用する。
use super::surface_flight::Overlay;
use eframe::egui::{self, Color32, Pos2, Rect, TextureId};
impl Overlay {
    /// Height fraction and perspective vertical field on the physical viewport.
    fn scales(self) -> (f64, f64, f64) {
        (
            (180. / self.camera.zoom).to_radians(),
            (self.fov * 0.5).to_radians().tan(),
            f64::from(self.rect.width()) / f64::from(self.rect.height()),
        )
    }
    fn inverse_x(self, u: f64) -> (f64, f64) {
        let (f, t, aspect) = self.scales();
        let k = t * aspect;
        (
            (1. - self.blend) * u * f + self.blend * (u * k).atan(),
            (1. - self.blend) * f + self.blend * k / (1. + (u * k).powi(2)),
        )
    }
    fn inverse_y(self, u: f64, v: f64) -> (f64, f64) {
        let (f, t, aspect) = self.scales();
        let k = t / (1. + (u * t * aspect).powi(2)).sqrt();
        (
            (1. - self.blend) * v * f * 0.5 + self.blend * (v * k).atan(),
            (1. - self.blend) * f * 0.5 + self.blend * k / (1. + (v * k).powi(2)),
        )
    }
    pub(crate) fn cylinder_uv(self, [x, y]: [f64; 2]) -> [f64; 2] {
        let u = 2. * x - 1.;
        let v = 1. - 2. * y;
        let f = self.scales().0;
        [
            0.5 + self.inverse_x(u).0 / (2. * f),
            0.5 - self.inverse_y(u, v).0 / f,
        ]
    }
    pub(crate) fn project(self, [x, y]: [f64; 2]) -> [f64; 2] {
        let f = self.scales().0;
        let u = solve((x - 0.5) * 2. * f, |u| self.inverse_x(u));
        let v = solve((0.5 - y) * f, |v| self.inverse_y(u, v));
        [0.5 * (1. + u), 0.5 * (1. - v)]
    }
    pub(crate) fn image(self, painter: &egui::Painter, id: TextureId, rect: Rect) {
        let mut mesh = egui::Mesh::with_texture(id);
        // 円筒の全面画像から同じ向きの色を得る。狭い画角での端の曲率も分割する。
        let rows = (rect.height() / 12.).ceil().clamp(1., 128.) as u32;
        let cols = (rect.width() / 12.).ceil().clamp(1., 128.) as u32;
        for j in 0..=rows {
            for i in 0..=cols {
                let xy = [
                    f64::from(i) / f64::from(cols),
                    f64::from(j) / f64::from(rows),
                ];
                let uv = self.cylinder_uv(xy);
                mesh.vertices.push(egui::epaint::Vertex {
                    pos: Pos2::new(
                        rect.left() + xy[0] as f32 * rect.width(),
                        rect.top() + xy[1] as f32 * rect.height(),
                    ),
                    uv: Pos2::new(uv[0] as f32, uv[1] as f32),
                    color: Color32::WHITE,
                });
                if i > 0 && j > 0 {
                    let k = j * (cols + 1) + i;
                    mesh.add_triangle(k - cols - 2, k - cols - 1, k);
                    mesh.add_triangle(k - cols - 2, k, k - 1);
                }
            }
        }
        painter.add(mesh);
    }
}
fn solve(target: f64, at: impl Fn(f64) -> (f64, f64)) -> f64 {
    let (edge, slope) = at(1.);
    if target.abs() > edge {
        return target.signum() * (1. + (target.abs() - edge) / slope);
    }
    let mut left = -1.;
    let mut right = 1.;
    let mut value = target / edge;
    for _ in 0..32 {
        let (v, slope) = at(value);
        let err = v - target;
        if err.abs() <= 1e-12 * edge {
            break;
        }
        if err < 0. {
            left = value
        } else {
            right = value
        }
        let next = value - err / slope;
        value = if next > left && next < right {
            next
        } else {
            (left + right) * 0.5
        };
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::projection::{Camera, View};
    #[test]
    fn native_rolled_horizon_pixels_match_during_the_entire_blend() {
        let camera = Camera {
            longitude: 214.,
            latitude: 17.,
            roll: -21.,
            zoom: 1.5,
        };
        let rect = Rect::from_min_max(Pos2::ZERO, Pos2::new(960., 480.));
        for (blend, a, b) in [
            (
                0.,
                [92.971513858857, 57.865427004654],
                [286.96830306433, -60.224214822175],
            ),
            (
                0.2,
                [111.47517418483, 57.455160805015],
                [276.83339140094, -51.149486266526],
            ),
            (
                0.65,
                [151.83841678251, 51.267196099568],
                [257.6208609462, -29.600921777326],
            ),
            (
                1.,
                [177.33059128751, 41.499797754432],
                [243.67191593201, -12.375935381572],
            ),
        ] {
            let f = Overlay {
                camera,
                rect,
                blend,
                fov: 53.,
            };
            for (ad, click) in [
                (a, [115.5, 72.5]),
                ([214., 17.], [480., 240.]),
                (b, [874.5, 400.5]),
            ] {
                let uv = camera.position(View::Horizon, ad);
                let xy = f.project(uv);
                let inverse = f.cylinder_uv([click[0] / 960., click[1] / 480.]);
                assert!(
                    (xy[0] * 960. - click[0]).abs() < 1e-8
                        && (xy[1] * 480. - click[1]).abs() < 1e-8
                );
                assert!((uv[0] - inverse[0]).abs() < 1e-10 && (uv[1] - inverse[1]).abs() < 1e-10);
            }
        }
    }
}
