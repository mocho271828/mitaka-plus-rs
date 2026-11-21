//! Mitaka_plus f9fe624 HipSpatialRenderer.mm, surface_star_pick.hpp, constellation_pick.cpp.
use super::{clip_line, dot, scale, Point, SolarCamera};
use crate::{catalog::SkyData, layers::LayerOptions};
use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Stroke};
use mitaka_core::{
    astronomy::{equatorial_direction, j2000_equatorial_to_ecliptic},
    catalog::Star,
};

use super::precise::AU_PER_PC;

fn direction([ra, dec]: [f64; 2]) -> Option<Point> {
    equatorial_direction(ra, dec)
        .and_then(j2000_equatorial_to_ecliptic)
        .ok()
}
fn angles(star: &Star) -> [f64; 2] {
    [star.ra_deg.into(), star.dec_deg.into()]
}

/// 距離が決まらない原行も、各惑星の地表から見る星空には残す。
pub(crate) struct Catalog {
    directions: Vec<Option<Point>>,
    spatial: Vec<(usize, Point)>,
}
impl Catalog {
    pub(crate) fn new(stars: &[Star]) -> Self {
        let directions: Vec<_> = stars.iter().map(|s| direction(angles(s))).collect();
        let spatial = stars
            .iter()
            .zip(&directions)
            .enumerate()
            .filter_map(|(index, (star, dir))| {
                let distance = 1000. / f64::from(star.parallax_mas);
                (star.parallax_mas.is_finite()
                    && star.parallax_mas > 0.
                    && distance <= 1e40
                    && (0. ..360.).contains(&star.ra_deg))
                .then(|| dir.map(|v| (index, scale(v, distance))))?
            })
            .collect();
        Self {
            directions,
            spatial,
        }
    }
}
#[derive(Clone, Copy)]
pub(super) struct Projector {
    basis: [Point; 3],
    center: Pos2,
    focal: f64,
    camera: SolarCamera,
}
impl Projector {
    fn new(camera: SolarCamera, rect: Rect) -> Self {
        Self {
            basis: camera.basis(),
            center: rect.center(),
            focal: f64::from(rect.height()) / (2. * (camera.fov * 0.5).to_radians().tan()),
            camera,
        }
    }
    fn point(self, delta: Point, distance: f64) -> Option<(Pos2, f64)> {
        let depth = distance - dot(delta, self.basis[2]);
        if depth <= distance * 1e-4 {
            return None;
        }
        let k = self.focal / depth;
        let xy = self.center
            + egui::vec2(
                (dot(delta, self.basis[0]) * k) as f32,
                (-dot(delta, self.basis[1]) * k) as f32,
            );
        xy.is_finite().then_some((xy, depth))
    }
    fn apparent(self, direction: Point) -> Option<Pos2> {
        self.point(direction, self.camera.distance / 1e6)
            .map(|p| p.0)
    }
    fn spatial(self, pos_pc: Point) -> Option<(Pos2, f64)> {
        self.point(
            self.camera.anchor().relative_pc(pos_pc),
            self.camera.distance / AU_PER_PC,
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Selection {
    Star(i32),
    Constellation(usize),
}
#[derive(Clone, Copy)]
pub(crate) struct Display {
    pub magnitude: f32,
    pub lines: bool,
    pub labels: bool,
    pub selected: Option<i32>,
}
struct Hit {
    object: Selection,
    center: Pos2,
    distance: f32,
    depth: f64,
    painted: bool,
}
#[derive(Default)]
pub(super) struct Hits(Vec<Hit>);
impl Hits {
    pub fn pick(&self, click: Pos2, opaque: impl Fn(Pos2) -> bool) -> Option<Selection> {
        if opaque(click) {
            return None;
        }
        let mut first = None::<&Hit>;
        for candidate in self.0.iter().filter(|h| !opaque(h.center)) {
            let score = |h: &Hit| match h.object {
                Selection::Constellation(_) => 0,
                Selection::Star(_) if h.painted => 2,
                Selection::Star(_) => 1,
            };
            if first.is_none_or(|old| {
                score(candidate) > score(old)
                    || score(candidate) == score(old)
                        && (candidate.painted
                            || candidate.distance < old.distance
                            || candidate.distance == old.distance && candidate.depth <= old.depth)
            }) {
                first = Some(candidate)
            }
        }
        first.map(|h| h.object)
    }
}

pub(super) struct Sky<'a> {
    pub data: &'a SkyData,
    pub layers: &'a LayerOptions,
    pub display: Display,
    pub selected: Option<Selection>,
    pub apparent: bool,
    pub preview: bool,
}
impl Sky<'_> {
    pub fn draw(
        &self,
        painter: &egui::Painter,
        rect: Rect,
        camera: SolarCamera,
        click: Option<Pos2>,
    ) -> Hits {
        let project = Projector::new(camera, rect);
        let mut hits = Hits::default();
        if self.apparent {
            self.constellations(painter, rect, project, click, &mut hits);
        }
        if !self.layers.stars {
            return hits;
        }
        let mut plot = |index: usize, pos: Pos2, depth: f64| {
            if !rect.contains(pos) {
                return;
            }
            let s = &self.data.stars[index];
            let object = Selection::Star(s.hip);
            let mut painted = false;
            if self.apparent {
                let (radius, alpha) = self.layers.point_style(s.magnitude);
                let warm = s.bv.clamp(0., 1.6) / 1.6;
                let color = Color32::from_rgba_unmultiplied(
                    ((0.78 + 0.22 * warm) * 255.) as u8,
                    ((0.87 - 0.12 * warm) * 255.) as u8,
                    ((1. - 0.4 * warm) * 255.) as u8,
                    (alpha * 255.) as u8,
                );
                painter.circle_filled(pos, radius, color);
                if self.layers.marking == 1 && s.is_double
                    || self.layers.marking == 2 && self.data.layers.hosts.contains(&s.hip)
                {
                    painter.circle_stroke(
                        pos,
                        6.,
                        Stroke::new(
                            1_f32,
                            if self.layers.marking == 1 {
                                Color32::LIGHT_GREEN
                            } else {
                                Color32::YELLOW
                            },
                        ),
                    );
                }
                if self.layers.proper_names
                    || self.layers.flamsteed
                    || self.layers.bayer
                    || self.layers.hip
                {
                    let label = self.layers.name(&self.data.layers, s);
                    if !label.is_empty() {
                        painter.text(
                            pos + egui::vec2(radius + 2., -radius),
                            Align2::LEFT_BOTTOM,
                            label,
                            FontId::proportional(11.),
                            Color32::LIGHT_GRAY,
                        );
                    }
                }
            } else {
                let color = Color32::from_white_alpha(204);
                if self.preview {
                    let mark = Rect::from_min_size(pos, egui::Vec2::splat(1.5));
                    painter.rect_filled(mark, 0., color);
                    painted = click.is_some_and(|p| mark.contains(p));
                } else {
                    painter.circle_filled(pos, 0.75, color);
                    painted = click.is_some_and(|p| p.distance_sq(pos) <= 0.75_f32.powi(2));
                }
            }
            if self.display.selected == Some(s.hip) {
                painter.circle_stroke(pos, 6., Stroke::new(1_f32, Color32::GOLD));
            }
            if let Some(distance) = click.map(|p| p.distance(pos)).filter(|&d| d <= 8.) {
                hits.0.push(Hit {
                    object,
                    center: pos,
                    distance,
                    depth,
                    painted,
                });
            }
        };
        if self.apparent {
            for (i, d) in self.data.physical_sky.directions.iter().enumerate() {
                if self.data.stars[i].magnitude <= self.display.magnitude {
                    if let Some(p) = d.and_then(|d| project.apparent(d)) {
                        plot(i, p, 0.);
                    }
                }
            }
        } else {
            let catalog = &self.data.physical_sky.spatial;
            let step = if self.preview {
                catalog.len().div_ceil(6000).max(1)
            } else {
                1
            };
            for &(i, pos) in catalog.iter().step_by(step) {
                if let Some((p, depth)) = project.spatial(pos) {
                    plot(i, p, depth);
                }
            }
        }
        hits
    }
    fn constellations(
        &self,
        painter: &egui::Painter,
        rect: Rect,
        project: Projector,
        click: Option<Pos2>,
        hits: &mut Hits,
    ) {
        let opts = self.layers;
        let project = |p| direction(p).and_then(|d| project.apparent(d));
        let mut segment = |a: [f64; 2], b: [f64; 2], color: Color32, id: Option<usize>| {
            let delta = [(b[0] - a[0] + 180.).rem_euclid(360.) - 180., b[1] - a[1]];
            let n = (delta[0].abs().max(delta[1].abs()) / 2.).ceil().max(1.) as usize;
            let mut prev = project(a);
            for i in 1..=n {
                let t = i as f64 / n as f64;
                let next = project([a[0] + t * delta[0], a[1] + t * delta[1]]);
                if let (Some(p), Some(q)) = (prev, next) {
                    if let Some([u, v]) = clip_line(rect, p, q) {
                        painter.line_segment([u, v], Stroke::new(0.8_f32, color));
                        if let Some((index, mouse)) = id.zip(click) {
                            let d = v - u;
                            let frac = if d.length_sq() > 0. {
                                ((mouse - u).dot(d) / d.length_sq()).clamp(0., 1.)
                            } else {
                                0.
                            };
                            let center = u + d * frac;
                            let distance = mouse.distance(center);
                            if distance <= 6. {
                                hits.0.push(Hit {
                                    object: Selection::Constellation(index),
                                    center,
                                    distance,
                                    depth: 0.,
                                    painted: false,
                                });
                            }
                        }
                    }
                }
                prev = next;
            }
        };
        if opts.illustrations {
            for r in self
                .data
                .layers
                .art
                .iter()
                .filter(|r| opts.shows(&r.abbreviation))
            {
                for pair in r.art.indices.windows(2) {
                    if let (Ok(a), Ok(b)) = (usize::try_from(pair[0]), usize::try_from(pair[1])) {
                        segment(
                            r.art.coordinates[a],
                            r.art.coordinates[b],
                            Color32::from_rgb(101, 92, 115),
                            None,
                        );
                    }
                }
            }
        }
        for (i, r) in self
            .data
            .constellations
            .iter()
            .enumerate()
            .filter(|(_, r)| opts.shows(&r.abbreviation))
        {
            let color = |c| {
                if self.selected == Some(Selection::Constellation(i)) {
                    Color32::GOLD
                } else {
                    c
                }
            };
            if self.display.lines {
                for path in &r.hip_lines {
                    for pair in path.windows(2) {
                        if let (Some(&a), Some(&b)) = (
                            self.data.by_hip.get(&pair[0]),
                            self.data.by_hip.get(&pair[1]),
                        ) {
                            segment(
                                angles(&self.data.stars[a]),
                                angles(&self.data.stars[b]),
                                color(Color32::from_rgb(58, 83, 116)),
                                Some(i),
                            );
                        }
                    }
                }
            }
            if opts.boundaries && r.boundary.len() > 1 {
                for (a, b) in r.boundary.iter().zip(r.boundary.iter().cycle().skip(1)) {
                    let p = |v: &[f32; 2]| [f64::from(v[0]) * 15., f64::from(v[1])];
                    segment(p(a), p(b), color(Color32::from_rgb(100, 85, 54)), Some(i));
                }
            }
            if self.display.labels {
                if let Some(pos) =
                    project([r.ra_deg.into(), r.dec_deg.into()]).filter(|&p| rect.contains(p))
                {
                    painter.text(
                        pos,
                        Align2::CENTER_CENTER,
                        String::from_utf8_lossy(&r.abbreviation),
                        FontId::proportional(11.),
                        color(Color32::from_rgb(109, 137, 168)),
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn apparent_matches_native_projection_with_current_camera() {
        let p = Projector::new(
            SolarCamera::default(),
            Rect::from_min_max(Pos2::ZERO, Pos2::new(640., 480.)),
        );
        for (ra, x, y) in [
            (165., 550.540024157, 351.327070207),
            (190., 392.033179037, 217.227223224),
            (225., 129.831372803, 118.426284073),
        ] {
            let actual = p.apparent(direction([ra, -40.]).unwrap()).unwrap();
            assert!((f64::from(actual.x) - x).hypot(f64::from(actual.y) - y) < 0.0001);
        }
    }
    #[test]
    fn picking_tests_both_click_and_drawn_centers_and_keeps_paint_order() {
        let p = Pos2::new(8., 8.);
        let mk = |object, x, painted| Hit {
            object,
            center: Pos2::new(x, 8.),
            distance: (p.x - x).abs(),
            depth: 1.,
            painted,
        };
        let hits = Hits(vec![
            mk(Selection::Constellation(0), 8., false),
            mk(Selection::Star(1), 8.4, true),
            mk(Selection::Star(2), 8.6, true),
        ]);
        assert_eq!(hits.pick(p, |_| false), Some(Selection::Star(2)));
        assert_eq!(
            hits.pick(p, |q| q.x > 8.1),
            Some(Selection::Constellation(0))
        );
        assert_eq!(hits.pick(p, |q| q == p), None);
    }
    #[test]
    #[ignore = "local Mitaka Plus 1.5.4 HIP data"]
    fn positive_parallax_rows_positions_and_first_projected_points_match_native() {
        let root = std::path::PathBuf::from(std::env::var_os("MITAKA_DATA_ROOT").unwrap());
        let stars = mitaka_core::catalog::load_hipparcos(&root.join("hip.dat")).unwrap();
        let catalog = Catalog::new(&stars);
        assert_eq!(
            (catalog.directions.len(), catalog.spatial.len()),
            (117956, 113943)
        );
        let rect = Rect::from_min_max(Pos2::ZERO, Pos2::new(640., 480.));
        let p = Projector::new(SolarCamera::default(), rect);
        let first: Vec<_> = catalog
            .spatial
            .iter()
            .filter_map(|&(i, position)| {
                let (xy, _) = p.spatial(position)?;
                rect.contains(xy).then_some((stars[i].hip, xy))
            })
            .take(4)
            .collect();
        for ((hip, xy), (wanted, x, y)) in first.into_iter().zip([
            (49043, 634.791306525, 472.128981625),
            (49079, 630.19166401, 472.47364499),
            (49084, 630.467824267, 472.158788455),
            (49145, 611.113049093, 479.164972774),
        ]) {
            assert_eq!(hip, wanted);
            assert!((f64::from(xy.x) - x).hypot(f64::from(xy.y) - y) < 0.0001);
        }
    }
}
