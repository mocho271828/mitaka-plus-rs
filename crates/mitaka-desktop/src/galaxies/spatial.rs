//! Mitaka_plus f9fe624 GalaxyProjector と pick_galaxy_spatial_row の移植。
use super::catalog::{Kind, Object, Objects};
use crate::solar::{
    precise::{Target, AU_PER_PC},
    SolarCamera,
};
use eframe::egui::{self, Color32, ColorImage};
use glam::DVec3;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

#[derive(Clone, Copy, PartialEq)]
pub(super) struct View {
    pub camera: SolarCamera,
    pub width: f32,
    pub height: f32,
    pub scale: f32,
    pub mask: u8,
}
pub(super) struct Projection {
    target: Target,
    axes: [DVec3; 3],
    distance: f64,
    focal: f64,
    center: [f64; 2],
}
impl Projection {
    pub fn new(v: View) -> Self {
        Self {
            target: v.camera.anchor(),
            axes: v.camera.basis().map(DVec3::from_array),
            distance: v.camera.distance / AU_PER_PC,
            focal: f64::from(v.height) * 0.5 / (v.camera.fov * 0.5).to_radians().tan(),
            center: [f64::from(v.width) * 0.5, f64::from(v.height) * 0.5],
        }
    }
    pub fn shape(&self, kind: Kind, p: &Object) -> Option<Disc> {
        let relative = DVec3::from_array(self.target.relative_pc(p.position));
        let depth = self.distance - relative.dot(self.axes[2]);
        if depth < self.distance * 1e-4 {
            return None;
        }
        let scale = self.focal / depth;
        let x = self.center[0] + relative.dot(self.axes[0]) * scale;
        let y = self.center[1] - relative.dot(self.axes[1]) * scale;
        let w = self.center[0] * 2.;
        let h = self.center[1] * 2.;
        if !x.is_finite() || !y.is_finite() {
            return None;
        }
        let min_radius = match kind {
            Kind::Clusters => 2.,
            Kind::MilkyWay => 0.75,
            _ => 1.25,
        };
        let r = (p.radius_pc * scale).max(min_radius);
        let dx = x - x.clamp(0., w);
        let dy = y - y.clamp(0., h);
        if dx.hypot(dy) > r
            || ((dx != 0. || dy != 0.) && !matches!(kind, Kind::Nearby | Kind::Clusters))
        {
            return None;
        }
        Some(Disc { x, y, r, depth })
    }
}
#[derive(Clone, Copy, Debug)]
pub(super) struct Disc {
    pub x: f64,
    pub y: f64,
    pub r: f64,
    pub depth: f64,
}
fn rgb(v: [f32; 3]) -> Color32 {
    let [r, g, b] = v.map(|v| (v * 255.).round() as u8);
    Color32::from_rgb(r, g, b)
}
fn color(kind: Kind, group: u8) -> Color32 {
    match kind {
        Kind::Clusters => rgb([1., 0.85, 0.4]),
        Kind::Nearby => rgb([0.5, 1., 0.7]),
        Kind::Quasars => rgb([1., 0.55, 0.75]),
        Kind::Sdss => [
            Color32::from_rgb(255, 221, 113),
            Color32::from_rgb(255, 230, 155),
            Color32::from_rgb(255, 237, 201),
            Color32::from_rgb(246, 242, 251),
            Color32::from_rgb(220, 243, 255),
        ][usize::from(group)],
        Kind::MilkyWay => [
            Color32::from_rgb(255, 254, 255),
            Color32::from_rgb(220, 253, 255),
            Color32::from_rgb(255, 251, 229),
            Color32::from_rgb(255, 237, 143),
        ][usize::from(group)],
    }
}
#[derive(Clone, Copy, PartialEq, Debug)]
pub(super) struct Id(pub Kind, pub usize);
struct Hit {
    id: Id,
    disc: Disc,
}
pub(super) struct Raster {
    pub image: ColorImage,
    hits: Vec<Hit>,
}
impl Raster {
    pub fn pick(&self, x: f64, y: f64, radius: f64) -> Option<Id> {
        let mut painted = None;
        let mut nearest: Option<&Hit> = None;
        let mut best = radius;
        for hit in &self.hits {
            let d = hit.disc;
            let distance = (x - d.x).hypot(y - d.y);
            if distance <= d.r {
                painted = Some(hit.id)
            }
            if distance > best
                || nearest.is_some_and(|prev| {
                    distance == best
                        && d.depth >= prev.disc.depth
                        && (d.x != prev.disc.x || d.y != prev.disc.y)
                })
            {
                continue;
            }
            nearest = Some(hit);
            best = distance;
        }
        painted.or_else(|| nearest.map(|h| h.id))
    }
    pub fn render(v: View, catalogs: &[(Kind, Arc<Objects>)], cancel: &AtomicBool) -> Option<Self> {
        let scale = v.scale.min(2048. / v.width.max(v.height)).max(0.01);
        let [w, h] = [v.width, v.height].map(|n| (n * scale).ceil().clamp(1., 2048.) as usize);
        let [sx, sy] = [
            w as f64 / f64::from(v.width),
            h as f64 / f64::from(v.height),
        ];
        let mut image = ColorImage::new([w, h], Color32::TRANSPARENT);
        let mut hits = Vec::new();
        let projector = Projection::new(v);
        for (kind, cat) in catalogs {
            for (idx, p) in cat.rows.iter().enumerate() {
                if idx % 256 == 0 && cancel.load(Ordering::Relaxed) {
                    return None;
                }
                let Some(d) = projector.shape(*kind, p) else {
                    continue;
                };
                let c = color(*kind, p.group);
                // 画面外の巨大円も画面サイズで打ち切る。行順が同一画素の前後を決定する。
                let span = |a: f64, r: f64, s: f64, max: usize| {
                    ((a - r) * s).floor().clamp(0., max as f64) as usize
                        ..((a + r) * s).ceil().clamp(0., max as f64) as usize
                };
                for y in span(d.y, d.r, sy, h) {
                    let dy = ((y as f64 + 0.5) / sy - d.y) / d.r;
                    if dy.abs() > 1. {
                        continue;
                    }
                    let half = d.r * (1. - dy * dy).max(0.).sqrt();
                    let x0 = ((d.x - half) * sx - 0.5).ceil().clamp(0., w as f64) as usize;
                    let x1 = (((d.x + half) * sx - 0.5).floor() + 1.).clamp(0., w as f64) as usize;
                    if x0 < x1 {
                        image.pixels[y * w + x0..y * w + x1].fill(c);
                    }
                }
                hits.push(Hit {
                    id: Id(*kind, idx),
                    disc: d,
                });
            }
        }
        Some(Self { image, hits })
    }
}
pub(super) fn dimensions(rect: egui::Rect) -> [f32; 2] {
    [rect.width().max(1.), rect.height().max(1.)]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::galaxies::catalog::{nearby_radius, objects, sdss_class};
    fn object(position: [f64; 3], r: f64, row: usize) -> Object {
        Object {
            position,
            radius_pc: r,
            row,
            group: 0,
            name: None,
        }
    }
    fn view(camera: SolarCamera) -> View {
        View {
            camera,
            width: 960.,
            height: 540.,
            scale: 1.,
            mask: 31,
        }
    }
    #[test]
    fn native_shared_camera_projector_and_sub_au_pan() {
        // f9fe624 の frame_our_galaxy / GalaxyProjector / SharedCamera::pan の出力。
        let center = super::super::center();
        for (a, b) in
            center
                .into_iter()
                .zip([-437.25883960723877, -7949.690726841765, -781.8082695349208])
        {
            assert!((a - b).abs() < 2e-10);
        }
        let mut c = SolarCamera {
            yaw: 43.,
            pitch: -15.,
            roll: -23.,
            fov: 51.,
            distance: 40. * AU_PER_PC,
            ..Default::default()
        };
        c.set_anchor(Target {
            anchor_pc: [1e9, -2e9, 3e9],
            offset_au: [0.02, -0.006, 0.07],
        });
        let o = object([1e9 + 7., -2e9 + 5., 3e9 - 3.], 2.7, 1);
        let d = Projection::new(view(c)).shape(Kind::Clusters, &o).unwrap();
        for (a, b) in [d.x, d.y, d.depth, d.r].into_iter().zip([
            466.138624322012,
            289.5819658394327,
            30.984709679014045,
            49.32691961837073,
        ]) {
            assert!((a - b).abs() < 2e-11, "{a} {b}");
        }
        c.distance = 0.01;
        let [right, up, _] = c.basis();
        let scale = 2. * c.distance * (c.fov * 0.5).to_radians().tan() / 540.;
        c.pan_au(std::array::from_fn(|i| (-right[i] + 2. * up[i]) * scale));
        for (a, b) in c.precise().unwrap().offset_au.into_iter().zip([
            0.020009137953383025,
            -0.005994837277764898,
            0.0700380820111347,
        ]) {
            assert!((a - b).abs() < 1e-16);
        }
        assert_eq!(nearby_radius(1000., -1.), 0.);
        assert_eq!(nearby_radius(1000., 10800.), 0.);
        assert!((nearby_radius(1000., 30.) - 4.363323129985823).abs() < 1e-14);
        assert_eq!(
            [-1., -0.1, 0.1, 0.25, 0.51].map(sdss_class),
            [0, 1, 2, 3, 4]
        );
    }
    #[test]
    fn extended_discs_across_edges_and_last_painted_pick() {
        let c = SolarCamera {
            distance: 100. * AU_PER_PC,
            yaw: 0.,
            pitch: 0.,
            ..Default::default()
        };
        let v = view(c);
        let center = object([0., 0., 0.], 2.7, 1);
        let outside = object([0., -74., 0.], 4., 2);
        let r = Projection::new(v);
        assert!(r.shape(Kind::Sdss, &outside).is_none());
        assert!(r.shape(Kind::Nearby, &outside).is_some());
        let cat = Arc::new(Objects {
            rows: vec![center, outside],
            source_rows: 2,
            radius: 74.,
            radiation: None,
        });
        let raster = Raster::render(
            v,
            &[(Kind::Clusters, cat.clone()), (Kind::Nearby, cat)],
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(raster.pick(2., 270., 1.), Some(Id(Kind::Nearby, 1)));
        assert_eq!(raster.pick(480., 270., 1.), Some(Id(Kind::Nearby, 0)));
    }
    #[test]
    #[ignore = "同梱の全SDSS原行を投影・選択する"]
    fn shipped_sdss_rows_all_project_and_off_stride_row_can_be_picked() {
        let root = std::path::PathBuf::from(std::env::var_os("MITAKA_DATA_ROOT").unwrap());
        for (kind, count) in [(Kind::Sdss, 826360), (Kind::Quasars, 104848)] {
            let catalog = Arc::new(objects(kind, &root).unwrap());
            assert_eq!((catalog.rows.len(), catalog.source_rows), (count, count));
            let c = SolarCamera {
                distance: 6. * catalog.radius * AU_PER_PC,
                ..Default::default()
            };
            let raster =
                Raster::render(view(c), &[(kind, catalog.clone())], &AtomicBool::new(false))
                    .unwrap();
            assert_eq!(raster.hits.len(), count);
            let i = count - 2;
            let mut c = SolarCamera {
                distance: 1. * AU_PER_PC,
                ..Default::default()
            };
            c.set_target_pc(catalog.rows[i].position);
            let local =
                Raster::render(view(c), &[(kind, catalog.clone())], &AtomicBool::new(false))
                    .unwrap();
            assert_eq!(local.pick(480., 270., 2.), Some(Id(kind, i)));
            if kind == Kind::Sdss {
                let counts = catalog.rows.iter().fold([0usize; 5], |mut counts, p| {
                    counts[p.group as usize] += 1;
                    counts
                });
                eprintln!("SDSS eClass {counts:?}");
                assert_eq!(counts, [55622, 542126, 139256, 62694, 26662]);
            }
        }
    }
}
