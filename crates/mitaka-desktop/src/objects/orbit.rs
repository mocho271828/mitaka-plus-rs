//! Mitaka_plus f9fe624 SmallBodyRenderer.mm の現在位置付近の軌道再標本化。
use crate::solar::SolarCamera;
use eframe::egui::{Pos2, Rect};
use mitaka_core::small_body::{Orbit, Record};

type Point = [f64; 3];

#[derive(Clone, Copy)]
pub(super) struct Sample {
    pub jd: f64,
    pub position: Option<Point>,
}

pub(super) struct Curve<'a> {
    pub record: &'a Record,
    pub camera: SolarCamera,
    pub rect: Rect,
}

fn chord_distance(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let delta = b - a;
    let squared = delta.length_sq();
    if squared == 0. {
        return p.distance(a);
    }
    p.distance(a + ((p - a).dot(delta) / squared).clamp(0., 1.) * delta)
}

impl Curve<'_> {
    fn near_chord(&self, p: Point, a: Point, b: Point) -> bool {
        self.camera
            .project(p, self.rect)
            .zip(self.camera.project_segment(a, b, self.rect))
            .is_some_and(|((p, _), [(a, _), (b, _)])| chord_distance(p, a, b) <= 0.25)
    }

    fn crosses_view(&self, a: Point, b: Point) -> bool {
        self.camera
            .project_segment(a, b, self.rect)
            .is_some_and(|[a, b]| crate::solar::clip_line(self.rect, a.0, b.0).is_some())
    }

    fn subdivide(
        &self,
        a: (f64, Point),
        b: (f64, Point),
        depth: usize,
        emit: &mut impl FnMut(Point, Point),
    ) {
        let mid = a.0 + (b.0 - a.0) * 0.5;
        if mid <= a.0 || mid >= b.0 {
            emit(a.1, b.1);
            return;
        }
        let Ok(p) = self.record.position(mid) else {
            return;
        };
        if self.near_chord(p, a.1, b.1) {
            emit(a.1, b.1);
        } else if depth < 6 || self.crosses_view(a.1, p) || self.crosses_view(p, b.1) {
            // 原版の6段以降も、画面にかかる局所弧は投影誤差を満たすまで分割する。
            self.subdivide(a, (mid, p), depth + 1, emit);
            self.subdivide((mid, p), b, depth + 1, emit);
        }
    }

    fn phase(&self, samples: &[Sample], jd: f64, marker: Point) -> Option<(usize, f64)> {
        let Orbit::Mean(e) = self.record.orbit else {
            return None;
        };
        let first = samples.first()?.jd;
        let last = samples.last()?.jd;
        if !self
            .rect
            .contains(self.camera.project(marker, self.rect)?.0)
        {
            return None;
        }
        let t = if (first..=last).contains(&jd) {
            jd
        } else {
            first + (jd - first).rem_euclid(360. / e.mean_motion_deg_per_day)
        };
        let upper = samples.partition_point(|s| s.jd <= t);
        let edge = if upper == samples.len() {
            0
        } else {
            upper.saturating_sub(1)
        };
        Some((edge, t))
    }

    pub fn visit(
        &self,
        samples: &[Sample],
        jd: f64,
        marker: Point,
        mut emit: impl FnMut(Point, Point),
    ) {
        let phase = self.phase(samples, jd, marker);
        let edges = samples.len().saturating_sub(1);
        for (i, pair) in samples.windows(2).enumerate() {
            let [a, b] = [pair[0], pair[1]];
            let Some((p, q)) = a.position.zip(b.position) else {
                continue;
            };
            let Some((edge, t)) = phase
                .filter(|(e, _)| i == *e || i == (e + 1) % edges || i == (e + edges - 1) % edges)
            else {
                emit(p, q);
                continue;
            };
            if i == edge && a.jd < t && t < b.jd && !self.near_chord(marker, p, q) {
                self.subdivide((a.jd, p), (t, marker), 0, &mut emit);
                self.subdivide((t, marker), (b.jd, q), 0, &mut emit);
            } else {
                self.subdivide((a.jd, p), (b.jd, q), 0, &mut emit);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::{catalog::Catalog, display::Sampling, Kind};
    use super::*;
    use mitaka_core::small_body::Encoding;

    fn check(root: &std::path::Path, number: i32, encoding: Encoding, distance: f64) {
        let catalog = Catalog::load(Kind::Small, root).unwrap();
        let row = (0..catalog.len())
            .find(|&i| {
                catalog
                    .small_record(i)
                    .is_some_and(|r| r.encoding == encoding && r.number == Some(number))
            })
            .unwrap();
        let record = catalog.small_record(row).unwrap();
        let path = catalog
            .small_path(
                row,
                Sampling {
                    count: 129,
                    ..Default::default()
                },
            )
            .unwrap();
        let Orbit::Mean(e) = record.orbit else {
            panic!("expected a minor planet")
        };
        let rect = Rect::from_min_size(Pos2::ZERO, eframe::egui::vec2(800., 600.));
        for jd in [2451545., e.epoch_jd, path[0].jd] {
            let marker = record.position(jd).unwrap();
            let camera = SolarCamera {
                target: marker,
                yaw: 30.,
                pitch: 85.,
                distance,
                ..Default::default()
            };
            let curve = Curve {
                record,
                camera,
                rect,
            };
            let speed = glam::DVec3::from(record.position(jd + 0.01).unwrap())
                .distance(marker.into())
                / 0.01;
            let check_path = |samples: &[Sample], gap: bool| {
                let mut lines = Vec::new();
                curve.visit(samples, jd, marker, |a, b| {
                    if let Some([a, b]) = camera.project_segment(a, b, rect) {
                        lines.push((a.0, b.0));
                    }
                });
                let mut visible = 0;
                for j in -16..=16 {
                    let t = jd + distance / speed * f64::from(j) / 16.;
                    let p = record.position(t).unwrap();
                    let Some((screen, _)) =
                        camera.project(p, rect).filter(|(p, _)| rect.contains(*p))
                    else {
                        continue;
                    };
                    let error = lines
                        .iter()
                        .map(|&(a, b)| chord_distance(screen, a, b))
                        .fold(f32::INFINITY, f32::min);
                    assert!(
                        if gap { error > 3. } else { error < 0.6 },
                        "object {number} jd {jd} gap {gap}: {error} px"
                    );
                    visible += 1;
                }
                assert!(visible >= 3);
            };
            check_path(&path, false);
            if jd == 2451545. {
                let mut broken = path.to_vec();
                let (edge, _) = curve.phase(&broken, jd, marker).unwrap();
                broken[edge + 1].position = None;
                check_path(&broken, true);
            }
            assert_eq!(path.len(), 129);
        }
    }

    fn element_fixture(source: &[u8]) -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("orbits/asteroids");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("test.dat"), source).unwrap();
        root
    }

    #[test]
    fn sparse_orbits_keep_both_sides_of_the_marker_within_the_screen_error_limit() {
        let root = element_fixture(b"Name: Circle\nType: Asteroid\nNo: 1\nEpoch: 2451545\na: 1\nn: 1\ni: 0\nnode: 0\nw: 0\ne: 0\nM0: 0\n");
        let catalog = Catalog::load(Kind::Small, root.path()).unwrap();
        let record = catalog.small_record(0).unwrap();
        let jd = 2451545.;
        let marker = record.position(jd).unwrap();
        let camera = SolarCamera {
            target: marker,
            pitch: 90.,
            yaw: 0.,
            distance: 0.001,
            ..Default::default()
        };
        let rect = Rect::from_min_size(Pos2::ZERO, eframe::egui::vec2(800., 600.));
        let curve = Curve {
            record,
            camera,
            rect,
        };
        for count in [3, 2] {
            let path = catalog
                .small_path(
                    0,
                    Sampling {
                        count,
                        ..Default::default()
                    },
                )
                .unwrap();
            let mut lines = Vec::new();
            curve.visit(&path, jd, marker, |a, b| {
                if let Some([a, b]) = camera.project_segment(a, b, rect) {
                    lines.push((a.0, b.0));
                }
            });
            let mut worst = 0.0_f32;
            for step in -20..=20 {
                let phase = f64::from(step) * 0.00002;
                let p = record.position(jd + phase.to_degrees()).unwrap();
                let screen = camera.project(p, rect).unwrap().0;
                assert!(rect.contains(screen));
                let error = lines
                    .iter()
                    .map(|&(a, b)| chord_distance(screen, a, b))
                    .fold(f32::INFINITY, f32::min);
                worst = worst.max(error);
            }
            eprintln!("{count} samples: maximum screen error {worst} px");
            assert!(worst < 0.6, "{count} samples: {worst} px");
        }
    }
    #[test]
    fn zoomed_ellipse_follows_live_body_and_leaves_missing_interval_open() {
        let root = element_fixture(b"Name: Test\nType: Asteroid\nNo: 1\nEpoch: 2451546\na: 1.5\nn: 0.6\ni: 4\nnode: 15\nw: 5\ne: 0.4\nM0: 57\n");
        check(root.path(), 1, Encoding::ElementText, 0.001);
    }
    #[test]
    #[ignore = "利用者の mp.dat とセドナの軌道要素"]
    fn native_zoom_probes_for_ceres_and_sedna() {
        let root = std::path::PathBuf::from(std::env::var_os("MITAKA_DATA_ROOT").unwrap());
        check(&root, 1, Encoding::Packed36, 0.01);
        check(&root, 90377, Encoding::ElementText, 0.05);
    }
}
