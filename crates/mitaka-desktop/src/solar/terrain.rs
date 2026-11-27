//! Mitaka_plus f9fe624 terrain_surface.cpp. Positions are relative to the body in datum radii.
use super::{Point, SolarCamera, KM_PER_AU, RADII_KM};
use glam::{DMat3, DVec2, DVec3};
use mitaka_core::terrain::{unit_direction, Amplification, Bounds, Mesh, Terrain, Vertex};
use std::{collections::HashMap, io, path::Path};

pub(super) fn eye(camera: SolarCamera, center: Point, radius: f64) -> DVec3 {
    (DVec3::from(camera.target) - DVec3::from(center)
        + DVec3::from(camera.basis()[2]) * camera.distance)
        / radius
}
pub(super) fn near(camera: SolarCamera, center: Point, body: usize) -> bool {
    if crate::surface::terrain_file(body).is_none() {
        return false;
    }
    let distance = eye(camera, center, RADII_KM[body] / KM_PER_AU).length();
    distance > 0. && distance <= 1. / 6_f64.to_radians().tan()
}
#[derive(Clone, Copy)]
pub(super) struct Sample {
    pub distance: f64,
    pub normal: DVec3,
    pub uv: DVec2,
}
impl Default for Sample {
    fn default() -> Self {
        Self {
            distance: f64::INFINITY,
            normal: DVec3::ZERO,
            uv: DVec2::ZERO,
        }
    }
}
/// 欠測・破損した地形でも通常の天体描画と選択深度を保持する。
pub(super) struct Layer {
    source: Option<Source>,
    file: &'static str,
    pub warning: Option<String>,
}
impl Layer {
    pub fn open(root: &Path, body: usize) -> Self {
        let file = crate::surface::terrain_file(body).expect("terrain candidate has a source path");
        let result = Source::open(root, body);
        let warning = result
            .as_ref()
            .err()
            .filter(|e| e.kind() != io::ErrorKind::NotFound)
            .map(|e| format!("{file}: {e}（基準球面で表示）"));
        Self {
            source: result.ok(),
            file,
            warning,
        }
    }
    pub fn render(
        &mut self,
        view: &View,
        mode: Amplification,
        preview: bool,
    ) -> Option<Vec<Sample>> {
        // 描画の資源上限は視点に依存するので、次の視点では同じ地形を再評価する。
        let result = self.source.as_mut()?.render(view, mode, preview);
        self.warning = result
            .as_ref()
            .err()
            .map(|e| format!("{}: {e}（基準球面で表示）", self.file));
        result.ok()
    }
}
pub(super) struct Source {
    source: Terrain,
    unit_radius: f64,
    meshes: HashMap<(usize, usize), Mesh>,
    mode: Amplification,
    cache_bytes: usize,
}
impl Source {
    pub fn open(root: &Path, index: usize) -> io::Result<Self> {
        let file = crate::surface::terrain_file(index)
            .ok_or_else(|| io::Error::other("この天体の地形データには対応していません"))?;
        let mut source = Terrain::open(&root.join(file))?;
        let unit_radius = source.grid(0, 0)?.unit_m / source.radius_m();
        Ok(Self {
            source,
            unit_radius,
            meshes: Default::default(),
            mode: Default::default(),
            cache_bytes: 0,
        })
    }
    fn mesh(&mut self, patch: usize, level: usize) -> io::Result<&Mesh> {
        let key = (patch, level);
        if !self.meshes.contains_key(&key) {
            let grid = self.source.grid(patch, level)?;
            let size = (grid.columns + 1) * (grid.rows + 1) * size_of::<Vertex>();
            if size > 8 * 1024 * 1024 {
                return Err(io::Error::other("terrain patch exceeds 8 MiB"));
            }
            if self.cache_bytes + size > 64 * 1024 * 1024 {
                self.meshes.clear();
                self.cache_bytes = 0;
            }
            self.meshes.insert(key, grid.mesh(self.mode)?);
            self.cache_bytes += size;
        }
        Ok(&self.meshes[&key])
    }
    pub fn render(
        &mut self,
        view: &View,
        mode: Amplification,
        preview: bool,
    ) -> io::Result<Vec<Sample>> {
        self.source.verify_unchanged()?;
        if self.mode != mode {
            self.mode = mode;
            self.meshes.clear();
            self.cache_bytes = 0;
        }
        let [w, h] = view.size;
        let mut samples = vec![Sample::default(); w * h];
        let relief = 32767. * self.unit_radius * mode.factor() + 0.002 / (view.radius * KM_PER_AU);
        let eye = view.eye;
        let alt = eye.length() - 1.;
        let detail_eye = if alt >= 2. * relief {
            eye
        } else {
            eye.normalize()
                * if alt <= relief {
                    1.
                } else {
                    1. + alt * (alt - relief) / relief
                }
        };
        let mut triangles = 0;
        let mut budget = 64 * 1024 * 1024;
        for patch in 0..self.source.patch_count() {
            let bounds = self.source.bounds(patch)?;
            if view.outside(bounds, 32768. * self.unit_radius * mode.factor()) {
                continue;
            }
            let level = bounds.level(detail_eye.to_array(), preview)?;
            let mesh = self.mesh(patch, level)?;
            triangles += mesh.rows * mesh.columns * 2;
            if triangles > 8 * 1024 * 1024 {
                return Err(io::Error::other("terrain exceeds 8M triangles"));
            }
            let verts: Vec<_> = mesh
                .vertices
                .iter()
                .map(|v| {
                    let local = DVec3::from(v.position) - view.eye;
                    let cam = view.camera_axes.transpose() * local;
                    Corner {
                        cam,
                        screen: view.project(cam),
                        normal: DVec3::from(v.normal),
                        uv: v.uv.into(),
                    }
                })
                .collect();
            for indices in mesh.triangles() {
                view.triangle(indices.map(|i| verts[i]), &mut samples, &mut budget)?;
            }
        }
        self.source.verify_unchanged()?;
        Ok(samples)
    }
}
#[derive(Clone, Copy)]
struct Corner {
    cam: DVec3,
    screen: Option<DVec2>,
    normal: DVec3,
    uv: DVec2,
}
pub(super) struct View {
    eye: DVec3,
    camera_axes: DMat3,
    size: [usize; 2],
    pixel_center: DVec2,
    focal: DVec2,
    near: f64,
    radius: f64,
    planes: [DVec3; 4],
}
impl View {
    pub fn new(
        camera: SolarCamera,
        center: Point,
        axes: [Point; 3],
        radius: f64,
        size: [usize; 2],
        aspect: f64,
    ) -> Self {
        let fixed = DMat3::from_cols_array_2d(&axes);
        let basis = camera.basis();
        let camera_axes = fixed.transpose()
            * DMat3::from_cols(basis[0].into(), basis[1].into(), -DVec3::from(basis[2]));
        let local = fixed.transpose() * eye(camera, center, radius);
        let pixel_center = DVec2::new(size[0] as f64, size[1] as f64) * 0.5;
        let tangent = (camera.fov * 0.5).to_radians().tan();
        let focal = pixel_center / DVec2::new(tangent * aspect, tangent);
        let planes = [
            camera_axes.x_axis + camera_axes.z_axis * (tangent * aspect),
            -camera_axes.x_axis + camera_axes.z_axis * (tangent * aspect),
            camera_axes.y_axis + camera_axes.z_axis * tangent,
            -camera_axes.y_axis + camera_axes.z_axis * tangent,
        ];
        let near = camera.near_plane();
        Self {
            eye: local,
            camera_axes,
            size,
            pixel_center,
            focal,
            radius,
            planes,
            near: near / radius,
        }
    }
    fn outside(&self, b: Bounds, margin: f64) -> bool {
        if margin >= 1. || b.west < -360. || b.east > 360. {
            return false;
        }
        let mid = DVec3::from(unit_direction(
            (b.west + b.east) * 0.5,
            (b.south + b.north) * 0.5,
        ));
        let angle =
            (0.5 * (b.east - b.west + b.north - b.south).to_radians()).min(std::f64::consts::PI);
        let chord = 2. * (angle * 0.5).sin();
        self.planes.iter().any(|&p| {
            let len = p.length();
            let upper = (mid.dot(p) + len * chord).min(len);
            upper * (1. + margin * upper.signum()) - self.eye.dot(p)
                < -1e-12 * (1. + len + self.eye.length() * len)
        })
    }
    fn project(&self, p: DVec3) -> Option<DVec2> {
        (p.z >= self.near).then(|| self.to_pixel(p))
    }
    fn to_pixel(&self, p: DVec3) -> DVec2 {
        self.pixel_center + self.focal * DVec2::new(p.x, -p.y) / p.z
    }
    fn triangle(
        &self,
        vertices: [Corner; 3],
        out: &mut [Sample],
        budget: &mut usize,
    ) -> io::Result<()> {
        if vertices.iter().all(|v| v.screen.is_none()) {
            return Ok(());
        }
        let mut min = DVec2::splat(f64::INFINITY);
        let mut max = -min;
        let mut add = |p: DVec2| {
            min = min.min(p);
            max = max.max(p);
        };
        for i in 0..3 {
            let (a, b) = (vertices[i], vertices[(i + 1) % 3]);
            if let Some(p) = a.screen {
                add(p);
            }
            if a.screen.is_some() != b.screen.is_some() {
                let t = (self.near - a.cam.z) / (b.cam.z - a.cam.z);
                let mut cut = a.cam.lerp(b.cam, t);
                cut.z = self.near;
                add(self.to_pixel(cut));
            }
        }
        let min = (min - DVec2::splat(0.5)).ceil().max(DVec2::ZERO);
        let max = (max - DVec2::splat(0.5))
            .floor()
            .min(self.pixel_center * 2. - DVec2::ONE);
        if min.x > max.x || min.y > max.y {
            return Ok(());
        }
        let (x0, y0, x1, y1) = (
            min.x as usize,
            min.y as usize,
            max.x as usize,
            max.y as usize,
        );
        *budget = budget
            .checked_sub((x1 - x0 + 1) * (y1 - y0 + 1))
            .ok_or_else(|| io::Error::other("terrain exceeds 64M pixel tests"))?;
        let [a, b, c] = vertices;
        let ab = b.cam - a.cam;
        let ac = c.cam - a.cam;
        let normal = ab.cross(ac);
        let norm2 = normal.length_squared();
        if norm2 == 0. {
            return Ok(());
        }
        let u = ac.cross(normal) / norm2;
        let v = normal.cross(ab) / norm2;
        let numerator = normal.dot(a.cam);
        for y in y0..=y1 {
            for x in x0..=x1 {
                let s =
                    (DVec2::new(x as f64 + 0.5, y as f64 + 0.5) - self.pixel_center) / self.focal;
                let ray = DVec3::new(s.x, -s.y, 1.);
                let depth = numerator / ray.dot(normal);
                if !depth.is_finite() || depth < self.near {
                    continue;
                }
                let p = ray * depth - a.cam;
                let wb = u.dot(p);
                let wc = v.dot(p);
                let wa = 1. - wb - wc;
                if wa < -1e-12 || wb < -1e-12 || wc < -1e-12 {
                    continue;
                }
                let d = depth * ray.length() * self.radius;
                let pixel = &mut out[y * self.size[0] + x];
                if d < pixel.distance {
                    *pixel = Sample {
                        distance: d,
                        normal: (a.normal * wa + b.normal * wb + c.normal * wc).normalize(),
                        uv: a.uv * wa + b.uv * wb + c.uv * wc,
                    };
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const AXES: [Point; 3] = [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]];
    #[test]
    fn optional_moon_and_mercury_sources_supply_heights_and_surface_depth() {
        use crate::solar::{sphere, Scene};
        use eframe::egui::{self, Pos2, Rect};
        let jd = crate::J2000;
        for (body, file) in [(10, "moon_topo.dat"), (1, "mercury_topo.dat")] {
            let root = tempfile::tempdir().unwrap();
            let radius = RADII_KM[body] / KM_PER_AU;
            let mut scene = Scene {
                jd,
                bodies: [[0.; 3]; 11],
                orbits: vec![],
                jupiter_casters: vec![],
                eclipse_warnings: vec![],
                satellite_positions_checked: true,
            };
            scene.bodies[body] = [0.5, 0.3, 0.01];
            let axes = if body == 10 {
                mitaka_core::lunar::lunar_orientation(jd).unwrap()
            } else {
                mitaka_core::rotation::planet_orientation(0, jd).unwrap()
            };
            let camera = SolarCamera {
                target: scene.bodies[body],
                frame: Some([axes[1], axes[2], axes[0]]),
                distance: 2. * radius,
                ..Default::default()
            };
            let rect = Rect::from_min_size(Pos2::ZERO, egui::vec2(65., 65.));
            for supplied in ["absent", "truncated", "ready"] {
                if supplied == "truncated" {
                    std::fs::write(root.path().join(file), [1u8, 2]).unwrap();
                } else if supplied == "ready" {
                    crate::smoke::terrain::write_tile(root.path(), body).unwrap();
                }
                let relief = crate::surface::Surface::load_optional(
                    root.path(),
                    body,
                    0.,
                    0.,
                    Amplification::Actual,
                );
                if supplied == "truncated" {
                    assert!(relief.is_err());
                } else {
                    assert_eq!(
                        relief.unwrap().map(|m| m.central_height),
                        (supplied == "ready").then_some(400.)
                    );
                }
                let mut surfaces = sphere::Surfaces::new();
                surfaces.eclipse.shadows = false;
                let ctx = egui::Context::default();
                let _ = ctx.run(egui::RawInput::default(), |ctx| {
                    let painter = ctx.layer_painter(egui::LayerId::background());
                    assert!(surfaces
                        .draw(
                            &painter,
                            root.path(),
                            camera,
                            &scene,
                            body,
                            sphere::Region {
                                viewport: rect,
                                tile: rect
                            }
                        )
                        .unwrap());
                });
                assert_eq!(
                    surfaces.warnings().any(|s| s.starts_with(file)),
                    supplied == "truncated"
                );
                let want = radius
                    - if supplied == "ready" {
                        0.4 / KM_PER_AU
                    } else {
                        0.
                    };
                let depth = surfaces.body_depth_at(body, rect.center());
                assert!(
                    (depth - want).abs() < 1e-13,
                    "{body}, {supplied}: {depth} vs {want}"
                );
            }
        }
    }
    #[test]
    fn six_degree_switch_matches_native_and_ignores_other_bodies() {
        for body in [1, 3, 4, 10] {
            let radius = RADII_KM[body] / KM_PER_AU;
            for (distance, want) in [(6., true), (9.51, true), (9.52, false), (0., false)] {
                let c = SolarCamera {
                    distance: distance * radius,
                    ..Default::default()
                };
                assert_eq!(near(c, [0.; 3], body), want);
            }
        }
        assert!(!near(SolarCamera::default(), [0.; 3], 5));
    }
    #[test]
    #[ignore = "local Mitaka Plus 1.5.4 signed Earth and Mars elevation tiles"]
    fn native_coverage_depth_texture_and_tangent_relief_at_all_approach_distances() {
        let root = std::path::PathBuf::from(std::env::var_os("MITAKA_DATA_ROOT").unwrap());
        for (body, n6, depth6, depth102, normal6, normal102, tangents) in [
            (
                3,
                305,
                0.00021317593526173,
                8.5270419560859e-7,
                [0.7276825407, 0.5030209787, 0.4663132154],
                [0.7232683257, 0.5064379338, 0.4694715628],
                [
                    (1.0000003135712, 1638, [24, 24, 24, 24, 23]),
                    (1.0000003135712, 2622, [15, 11, 11, 7, 0]),
                ],
            ),
            (
                4,
                307,
                0.00011351782602708,
                4.5855490752662e-7,
                [0.7252460394, 0.5099461021, 0.4625723245],
                [0.7109625016, 0.5258115156, 0.466963137],
                [
                    (0.99980182564432, 1622, [23, 23, 24, 25, 25]),
                    (0.99602532387754, 2383, [0, 1, 10, 23, 25]),
                ],
            ),
        ] {
            let mut source = Source::open(&root, body).unwrap();
            let radius = RADII_KM[body] / KM_PER_AU;
            let run = |source: &mut Source, c: SolarCamera, mode| {
                source
                    .render(
                        &View::new(c, [0.; 3], AXES, radius, [65, 49], 65. / 49.),
                        mode,
                        false,
                    )
                    .unwrap()
            };
            for (h, count, depth, normal) in [
                (6., n6, depth6, normal6),
                (1.02, 65 * 49, depth102, normal102),
            ] {
                let samples = run(
                    &mut source,
                    SolarCamera {
                        yaw: 35.,
                        pitch: 28.,
                        distance: radius * h,
                        ..Default::default()
                    },
                    Amplification::Actual,
                );
                let point = samples[24 * 65 + 32];
                assert_eq!(
                    samples.iter().filter(|q| q.distance.is_finite()).count(),
                    count
                );
                assert!(
                    (point.distance - depth).abs() < 1e-14,
                    "{body} {h} {depth} {}",
                    point.distance
                );
                assert!(point.normal.distance(normal.into()) < 1e-8);
                assert!(point.uv.distance(DVec2::new(35. / 360., 0.5 - 28. / 180.)) < 1e-9);
            }
            let radial = DVec3::from(unit_direction(35., 28.));
            let east = DVec3::from(unit_direction(125., 0.));
            let south = east.cross(radial);
            for ((h, count, ridge), mode) in tangents
                .into_iter()
                .zip([Amplification::Actual, Amplification::Twenty])
            {
                let c = SolarCamera {
                    distance: radius * h,
                    frame: Some([east.to_array(), radial.to_array(), south.to_array()]),
                    target: ((radial - south) * radius * h).to_array(),
                    ..Default::default()
                };
                let samples = run(&mut source, c, mode);
                assert_eq!(
                    samples.iter().filter(|q| q.distance.is_finite()).count(),
                    count,
                    "body {body} {mode:?}"
                );
                for (x, expect) in [0, 16, 32, 48, 64].into_iter().zip(ridge) {
                    assert_eq!(
                        (0..49).find(|&y| samples[y * 65 + x].distance.is_finite()),
                        Some(expect)
                    );
                }
            }
        }
    }
}
