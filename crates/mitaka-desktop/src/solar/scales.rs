//! 参照実装 distance_scales.cpp の距離目盛。光年の円は銀河面上に置く。
use super::{clip_line, plus, scale, Point, SolarCamera};
use eframe::egui::{self, Color32, Rect};
use mitaka_core::astronomy::j2000_equatorial_to_ecliptic;
const AU_M: f64 = 149597870700.;
const LY_AU: f64 = 299792458. * 365.25 * 86400. / AU_M;
fn circle_alpha(ratio: f64) -> f64 {
    ((ratio - 1.) / 4.).clamp(0., 1.) * ((70. - ratio) / 56.).clamp(0., 1.)
}
fn square_alpha(ratio: f64) -> f64 {
    0.9 * 1f64
        .min((ratio - 0.01) / 0.04)
        .min((5. - ratio) / 0.6)
        .max(0.)
}
pub(super) fn paint(
    p: &egui::Painter,
    rect: Rect,
    c: SolarCamera,
    opts: &crate::layers::LayerOptions,
    observer: [f64; 2],
    jd: f64,
) {
    let line = |a: Point, b: Point, color| {
        if let (Some((a, _)), Some((b, _))) = (c.project(a, rect), c.project(b, rect)) {
            if let Some(points) = clip_line(rect, a, b) {
                p.line_segment(points, egui::Stroke::new(0.8_f32, color));
            }
        }
    };
    let label = |a: Point, name: String, color| {
        if let Some((pos, _)) = c.project(a, rect) {
            if rect.contains(pos) {
                p.text(
                    pos,
                    egui::Align2::LEFT_BOTTOM,
                    name,
                    egui::FontId::proportional(11.),
                    color,
                );
            }
        }
    };
    use crate::layers::grid::{lines, System};
    use mitaka_core::astronomy::{equatorial_direction, ObservationFrame};
    let frame = ObservationFrame::new(observer[0], observer[1], jd).ok();
    let dir_camera = SolarCamera {
        frame: Some(c.basis()),
        distance: 1.,
        fov: c.fov,
        ..Default::default()
    };
    let eye = c.basis()[2];
    for (system, enabled, color) in [
        (
            System::Equatorial,
            opts.equatorial,
            Color32::from_rgb(70, 91, 127),
        ),
        (
            System::Galactic,
            opts.galactic,
            Color32::from_rgb(129, 82, 140),
        ),
        (
            System::Horizontal,
            opts.horizontal,
            Color32::from_rgb(76, 115, 84),
        ),
    ] {
        if !enabled {
            continue;
        }
        for path in lines(system) {
            let mut last = None;
            for &coord in path {
                let to_screen = || {
                    let [ra, dec] = if system == System::Horizontal {
                        frame.as_ref()?.to_equatorial(coord[0], coord[1]).ok()?
                    } else {
                        coord
                    };
                    let dir =
                        j2000_equatorial_to_ecliptic(equatorial_direction(ra, dec).ok()?).ok()?;
                    dir_camera.project(plus(eye, dir), rect).map(|(p, _)| p)
                };
                let next = to_screen();
                if let Some(segment) = last.zip(next).and_then(|(a, b)| clip_line(rect, a, b)) {
                    p.line_segment(segment, egui::Stroke::new(0.6_f32, color));
                }
                last = next;
            }
        }
    }
    if opts.ecliptic {
        let color = Color32::from_rgb(150, 116, 61);
        let corners = [
            [-100., -100., 0.],
            [100., -100., 0.],
            [100., 100., 0.],
            [-100., 100., 0.],
            [-100., -100., 0.],
        ];
        for pair in corners.windows(2) {
            line(pair[0], pair[1], color);
        }
        line([-100., 0., 0.], [100., 0., 0.], color);
        line([0., -100., 0.], [0., 100., 0.], color);
    }
    if opts.circle_scales {
        let au = [1., 10., 100., 1000., 10000.];
        let ly = [
            1., 3., 10., 30., 100., 300., 1000., 3000., 1e4, 1e5, 1e6, 1e7, 1e8, 1e9, 1e10, 1.38e10,
        ];
        for (count, gal) in au
            .into_iter()
            .map(|r| (r, false))
            .chain(ly.into_iter().map(|r| (r, true)))
        {
            let r = if gal { count * LY_AU } else { count };
            let alpha = circle_alpha(c.distance / r);
            if alpha <= 0. {
                continue;
            }
            let point = |deg: f64| {
                let (s, cos) = deg.to_radians().sin_cos();
                let v = [r * cos, r * s, 0.];
                if gal {
                    j2000_equatorial_to_ecliptic(crate::layers::grid::galactic_to_j2000(v)).unwrap()
                } else {
                    v
                }
            };
            let angle = if gal && count == 1.38e10 {
                300.
            } else if !gal && count == 1. {
                230.
            } else {
                45.
            };
            let color = Color32::from_rgba_unmultiplied(120, 166, 194, (alpha * 0.65 * 255.) as u8);
            let mut last = point(0.);
            for a in (2..=360).step_by(2) {
                let next = point(f64::from(a));
                line(last, next, color);
                last = next;
            }
            label(
                point(angle),
                format!("{count} {}", if gal { "光年" } else { "AU" }),
                color,
            );
        }
    }
    if opts.square_scales {
        let [right, up, _] = c.basis();
        for exponent in 0..28 {
            let side = 10f64.powi(exponent) / AU_M;
            let a = square_alpha(side / c.distance);
            if a <= 0. {
                continue;
            }
            let points = [[-1., -1.], [1., -1.], [1., 1.], [-1., 1.], [-1., -1.]].map(|[x, y]| {
                plus(
                    c.target,
                    plus(scale(right, side * x / 2.), scale(up, side * y / 2.)),
                )
            });
            let color = Color32::from_rgba_unmultiplied(194, 181, 133, (a * 255.) as u8);
            for pair in points.windows(2) {
                line(pair[0], pair[1], color);
            }
            label(points[2], format!("10^{exponent} m"), color);
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn source_visibility_intervals() {
        assert_eq!(circle_alpha(70.), 0.);
        assert_eq!(circle_alpha(5.), 1.);
        assert_eq!(square_alpha(0.01), 0.);
        assert_eq!(square_alpha(1.), 0.9);
    }
}
