//! Mitaka_plus f9fe624 eclipse_display.cpp / eclipse_cone_renderer.cpp の物理表示。
use super::{dot, minus, plus, scale, Point, Scene, SolarCamera, KM_PER_AU, RADII_KM};
use eframe::egui::{self, Color32, ColorImage, Painter, Pos2, Rect, TextureHandle};
use mitaka_core::{
    eclipse::{
        display::{display_cones, Options, Surface},
        Sphere,
    },
    ephemeris::Cf96Ephemeris,
    satellite_catalog::{Dataset, SourceKind},
    satellite_model::Model,
};
use std::{io, path::Path};

pub(super) struct Source {
    model: Model,
    parents: [usize; 9],
    moon: usize,
    casters: Vec<(usize, f64)>,
    warnings: Vec<String>,
}
impl Source {
    pub fn load(root: &Path) -> io::Result<Self> {
        let dataset = Dataset::load(root, Default::default());
        let mut failures = vec![];
        for (i, source) in dataset
            .sources
            .iter()
            .enumerate()
            .filter(|(_, s)| s.kind == SourceKind::System)
        {
            let model = match Model::from_source(&dataset, i) {
                Ok(model) => model,
                Err(e) => {
                    failures.push(format!("{}: {e}", source.key));
                    continue;
                }
            };
            let sat: Vec<_> = source
                .records
                .iter()
                .filter(|r| r.block == "Satellite")
                .collect();
            if sat.iter().any(|r| r.name().is_none()) {
                failures.push(format!("{}: 衛星名が未確定", source.key));
                continue;
            }
            let parents: Vec<_> = source
                .records
                .iter()
                .filter(|r| r.block == "Planet")
                .map(|r| r.ordinal)
                .collect();
            let Ok(parents) = <[usize; 9]>::try_from(parents) else {
                continue;
            };
            let Some(moon) = sat.iter().find(|r| r.name() == Some(b"Moon")) else {
                continue;
            };
            if moon.parent != Some(parents[2]) {
                continue;
            }
            let mut casters = vec![];
            let mut warnings = vec![];
            for name in ["Io", "Europa", "Ganymede", "Calliso"] {
                // 原版の綴りと先頭登録を使う。別名や別出典の Radius を補わない。
                let row = sat.iter().find(|r| r.name() == Some(name.as_bytes()));
                let Some(r) = row else {
                    warnings.push(format!(
                        "{}: 木星の食の登録 {name} がありません",
                        source.key
                    ));
                    continue;
                };
                let radius = r
                    .field("Radius")
                    .and_then(|f| f.numbers.as_ref()?.first().copied())
                    .filter(|v| v.is_finite() && *v > 0.);
                if let Some(radius) = radius.filter(|_| r.parent == Some(parents[4])) {
                    casters.push((r.ordinal, radius / KM_PER_AU));
                } else {
                    warnings.push(format!("{}: {name} の親または物理半径が未確定", source.key));
                }
            }
            return Ok(Self {
                model,
                parents,
                moon: moon.ordinal,
                casters,
                warnings,
            });
        }
        Err(io::Error::other(format!(
            "木星の食に使える衛星出典がありません {}",
            failures.join("、")
        )))
    }
    pub fn apply(&self, scene: &mut Scene, eph: &Cf96Ephemeris) -> io::Result<()> {
        let frame = self.model.evaluate(eph, scene.jd)?;
        let mut bodies = scene.bodies;
        for (body, idx) in self.parents.into_iter().enumerate() {
            if let Ok(pos) = frame.position_au(idx) {
                bodies[body + 1] = pos;
            }
        }
        bodies[5] = frame.position_au(self.parents[4])?;
        bodies[10] = frame.position_au(self.moon)?;
        let mut casters = vec![];
        scene.eclipse_warnings.clone_from(&self.warnings);
        for &(row, radius_au) in &self.casters {
            match frame.position_au(row) {
                Ok(center_au) => casters.push(Sphere {
                    center_au,
                    radius_au,
                }),
                Err(error) => scene
                    .eclipse_warnings
                    .push(format!("{} 原行 {row}: {error}", self.model.source_key)),
            }
        }
        // 描画する親・月と遮蔽体を同一出典・同一時刻の評価結果にそろえる。
        scene.bodies = bodies;
        scene.jupiter_casters = casters;
        Ok(())
    }
}
fn sphere(scene: &Scene, body: usize) -> Sphere {
    Sphere {
        center_au: scene.bodies[body],
        radius_au: RADII_KM[body] / KM_PER_AU,
    }
}
pub fn surface(scene: &Scene, body: usize, options: Options) -> io::Result<Option<Surface>> {
    let single;
    let casters = match body {
        3 | 10 => {
            single = [sphere(scene, if body == 3 { 10 } else { 3 })];
            single.as_slice()
        }
        5 => scene.jupiter_casters.as_slice(),
        _ => return Ok(None),
    };
    Surface::bind(
        sphere(scene, 0),
        sphere(scene, body),
        casters,
        options,
        if body == 10 { [0.3, 0., 0.] } else { [0.; 3] },
    )
}
fn norm(a: Point) -> f64 {
    a[0].hypot(a[1]).hypot(a[2])
}
fn cross(a: Point, b: Point) -> Point {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn nearest_sphere(spheres: &[(Point, f64)], ray: Point) -> f64 {
    let mut nearest = f64::INFINITY;
    for &(eye, radius) in spheres {
        let axial = -dot(eye, ray);
        if axial <= 0. {
            continue;
        }
        let impact = norm(cross(eye, ray)) / radius;
        if impact <= 1. {
            nearest = nearest.min(axial - radius * ((1. - impact) * (1. + impact)).sqrt());
        }
    }
    nearest
}
fn raster(
    geometry: [Sphere; 3],
    opaque: &[Sphere],
    camera: SolarCamera,
    options: Options,
    [w, h]: [usize; 2],
) -> io::Result<ColorImage> {
    let [sun, occ, receiver] = geometry;
    let cones = display_cones(sun, occ, receiver, options)?;
    let mut pixels = vec![Color32::TRANSPARENT; w * h];
    let [right, up, out] = camera.basis();
    let eye = |center| plus(minus(camera.target, center), scale(out, camera.distance));
    let eyes: Vec<_> = cones.iter().map(|c| eye(c.apex)).collect();
    let spheres: Vec<_> = opaque
        .iter()
        .map(|s| (eye(s.center_au), s.radius_au))
        .collect();
    if cones.is_empty() || spheres.iter().any(|&(r, d)| norm(r) <= d) {
        return Ok(ColorImage {
            size: [w, h],
            pixels,
        });
    }
    let scale_px = 2. * (camera.fov.to_radians() * 0.5).tan() / h as f64;
    let hor: Vec<_> = (0..w)
        .map(|x| {
            minus(
                scale(right, (x as f64 + 0.5 - w as f64 * 0.5) * scale_px),
                out,
            )
        })
        .collect();
    for (y, row) in pixels.chunks_exact_mut(w).enumerate() {
        let row_up = scale(up, (h as f64 * 0.5 - y as f64 - 0.5) * scale_px);
        for (p, &x) in row.iter_mut().zip(&hor) {
            let v = plus(x, row_up);
            let ray = scale(v, 1. / norm(v));
            let mut trans = 1.;
            let mut limit = None;
            for (cone, &from) in cones.iter().zip(&eyes) {
                for (t, alpha) in cone.intersections(from, ray).into_iter().flatten() {
                    if t < *limit.get_or_insert_with(|| nearest_sphere(&spheres, ray)) {
                        trans *= 1. - alpha
                    }
                }
            }
            let a = ((1. - trans) * 255.).round() as u8;
            if a > 0 {
                *p = Color32::from_rgba_unmultiplied(255, 0, 0, a);
            }
        }
    }
    Ok(ColorImage {
        size: [w, h],
        pixels,
    })
}
#[derive(Default)]
pub(super) struct Overlay {
    cached: Option<(SolarCamera, f64, [usize; 2], Options, TextureHandle)>,
}
impl Overlay {
    pub fn draw(
        &mut self,
        painter: &Painter,
        rect: Rect,
        scene: &Scene,
        camera: SolarCamera,
        options: Options,
    ) -> io::Result<()> {
        if !(options.umbra || options.penumbra) || !rect.is_positive() {
            return Ok(());
        }
        let s = (800. / rect.width().max(rect.height())).min(1.);
        let size =
            [rect.width(), rect.height()].map(|dim| (dim * s).round().clamp(1., 800.) as usize);
        if self.cached.as_ref().is_none_or(|(c, j, d, o, _)| {
            *c != camera || *j != scene.jd || *d != size || *o != options
        }) {
            let opaque: Vec<_> = (0..scene.bodies.len()).map(|i| sphere(scene, i)).collect();
            let image = raster(
                [opaque[0], opaque[10], opaque[3]],
                &opaque,
                camera,
                options,
                size,
            )?;
            let texture =
                painter
                    .ctx()
                    .load_texture("食の影錐", image, egui::TextureOptions::LINEAR);
            self.cached = Some((camera, scene.jd, size, options, texture));
        }
        if let Some((_, _, _, _, texture)) = &self.cached {
            painter.image(
                texture.id(),
                rect,
                Rect::from_min_max(Pos2::ZERO, egui::pos2(1., 1.)),
                Color32::WHITE,
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cone_images_match_cpp_for_both_fans_and_opaque_spheres() {
        let body = |x, radius_au| Sphere {
            center_au: [x, 0., 0.],
            radius_au,
        };
        let geom = [body(0., 2.), body(10., 1.), body(14., 0.5)];
        let camera = SolarCamera {
            target: [12., 0., 0.],
            yaw: 90.,
            pitch: 0.,
            distance: 20.,
            ..Default::default()
        };
        let oracle = [
            (152, 12497, 11518965190793153642),
            (149, 11501, 10760584623066079052),
            (344, 52025, 9328267960583642952),
            (344, 51239, 5449518193886092550),
            (396, 59110, 4331092966493700429),
            (396, 58280, 2453288491380635591),
        ];
        let mut i = 0;
        for flags in [1, 2, 3] {
            for opaque in [&[][..], &geom[..]] {
                let options = Options {
                    umbra: flags & 1 != 0,
                    penumbra: flags & 2 != 0,
                    ..Default::default()
                };
                let out = raster(geom, opaque, camera, options, [51, 33]).unwrap();
                let mut n = 0;
                let mut sum = 0;
                let mut hash = 14695981039346656037_u64;
                for p in out.pixels {
                    let a = p.a();
                    n += usize::from(a != 0);
                    sum += u32::from(a);
                    hash = (hash ^ u64::from(a)).wrapping_mul(1099511628211);
                }
                assert_eq!(
                    (n, sum, hash),
                    oracle[i],
                    "mask {flags}, {} opaque spheres",
                    opaque.len()
                );
                i += 1;
            }
        }
    }
    #[test]
    #[ignore = "requires original Plus 1.5.4 satellite and CF96 files"]
    fn earth_moon_jupiter_share_the_original_registered_source() {
        let root = std::env::var_os("MITAKA_DATA_ROOT").unwrap();
        let root = Path::new(&root);
        let eph = Cf96Ephemeris::load(&root.join("cf96.dat")).unwrap();
        let source = Source::load(root).unwrap();
        let mut scene = Scene::load(&eph, 2460409.264).unwrap();
        source.apply(&mut scene, &eph).unwrap();
        assert_eq!(scene.jupiter_casters.len(), 3);
        assert!(scene.eclipse_warnings.iter().any(|w| w.contains("Calliso")));
        for (caster, expected) in scene.jupiter_casters.iter().zip([1815., 1569., 2631.]) {
            let d = norm(minus(scene.bodies[5], caster.center_au));
            assert!((0.001..0.01).contains(&d));
            assert!((caster.radius_au * KM_PER_AU - expected).abs() < 1e-9);
        }
        let earth = surface(&scene, 3, Options::default())
            .unwrap()
            .expect("April 8, 2024 solar eclipse");
        let hit = mitaka_core::eclipse::shadow_axis_hit(
            sphere(&scene, 0),
            sphere(&scene, 10),
            sphere(&scene, 3),
        )
        .unwrap()
        .unwrap();
        let n = minus(hit, scene.bodies[3]);
        let n = scale(n, 1. / norm(n));
        assert!(earth.effect(n, 0.).unwrap().multiplier[0] < 0.03);
    }
}
