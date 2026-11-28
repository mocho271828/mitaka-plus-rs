//! 現行CPU表面描画と同じ物理半径・テクセル中心・黄道基底を使う。
use super::*;
use eframe::egui::{ColorImage, TextureHandle, TextureOptions};
use image::{ImageReader, RgbImage};
use mitaka_core::terrain::Amplification;
use std::{collections::HashMap, path::Path};

pub(crate) const TEXTURE_NAMES: [&str; 11] = [
    "sun", "mercury", "venus", "earth", "mars", "jupiter", "saturn", "uranus", "neptune", "pluto",
    "moon",
];
const AU_KM: f64 = 149_597_870.7;
fn cross(a: Point, b: Point) -> Point {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn length(a: Point) -> f64 {
    a[0].hypot(a[1]).hypot(a[2])
}
fn unit(a: Point) -> Point {
    scale(a, 1. / length(a))
}
fn intersection(eye: Point, ray: Point) -> Option<Point> {
    if dot(eye, ray) >= 0. || dot(eye, eye) <= 1. {
        return None;
    }
    let h = cross(eye, ray);
    let discriminant = 1. - dot(h, h);
    if discriminant < -8. * f64::EPSILON {
        return None;
    }
    Some(unit(minus(
        cross(ray, h),
        scale(ray, discriminant.max(0.).sqrt()),
    )))
}
fn sample(image: &RgbImage, u: f64, v: f64) -> Point {
    let w = image.width();
    let h = image.height();
    let x = u.rem_euclid(1.) * f64::from(w) - 0.5;
    let y = (v * f64::from(h) - 0.5).clamp(0., f64::from(h - 1));
    let x0 = (x.floor() as i64).rem_euclid(i64::from(w)) as u32;
    let y0 = y.floor() as u32;
    let x1 = (x0 + 1) % w;
    let y1 = (y0 + 1).min(h - 1);
    let tx = x - x.floor();
    let ty = y - f64::from(y0);
    std::array::from_fn(|k| {
        let at = |x, y| f64::from(image.get_pixel(x, y)[k]);
        ((1. - tx) * at(x0, y0) + tx * at(x1, y0)) * (1. - ty)
            + ((1. - tx) * at(x0, y1) + tx * at(x1, y1)) * ty
    })
}
fn uv(normal: Point, axes: [Point; 3], offset: f64) -> (f64, f64) {
    (
        dot(normal, axes[1]).atan2(dot(normal, axes[0])) / std::f64::consts::TAU + offset,
        dot(normal, axes[2]).clamp(-1., 1.).acos() / std::f64::consts::PI,
    )
}
use mitaka_core::atmosphere::{self, EmissionMap, Parameters, Ray};
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Air {
    None,
    Single,
    #[default]
    Multiple,
}
impl Air {
    pub fn from_index(index: u64) -> Option<Self> {
        match index {
            0 => Some(Self::None),
            1 => Some(Self::Single),
            2 => Some(Self::Multiple),
            _ => None,
        }
    }
}
pub struct Region {
    pub viewport: Rect,
    pub tile: Rect,
}
struct Buffers<'a> {
    map: Option<&'a RgbImage>,
    emission: Option<&'a EmissionMap>,
    cloud: Option<&'a RgbImage>,
    lights: Option<&'a RgbImage>,
}
#[derive(Default)]
pub struct Surfaces {
    files: HashMap<&'static str, Result<RgbImage, String>>,
    cache: HashMap<usize, (Key, TextureHandle, Vec<f64>)>,
    terrains: HashMap<usize, terrain::Layer>,
    pub(super) mode: Amplification,
    pub(super) preview: bool,
    drawn: Vec<usize>,
    symbols: Vec<(Pos2, f32, f64)>,
    pub clouds: bool,
    pub lights: bool,
    pub eclipse: mitaka_core::eclipse::display::Options,
    pub air: Air,
    pub intensity: f64,
    emission: Option<Result<EmissionMap, String>>,
}
#[derive(PartialEq)]
struct Key {
    terrain: Option<(Amplification, bool)>,
    camera: SolarCamera,
    jd: f64,
    rect: Rect,
    tile: Rect,
    clouds: bool,
    lights: bool,
    eclipse: mitaka_core::eclipse::display::Options,
    air: Air,
    intensity: f64,
}
impl Surfaces {
    pub fn new() -> Self {
        Self {
            clouds: true,
            lights: false,
            intensity: 6.0,
            eclipse: Default::default(),
            ..Default::default()
        }
    }
    pub fn warnings(&self) -> impl Iterator<Item = &str> {
        self.files
            .values()
            .filter_map(|v| v.as_ref().err().map(String::as_str))
            .chain(
                self.emission
                    .iter()
                    .filter_map(|v| v.as_ref().err().map(String::as_str)),
            )
            .chain(
                self.terrains
                    .values()
                    .filter_map(|entry| entry.warning.as_deref()),
            )
    }
    pub(super) fn begin_frame(&mut self) {
        self.drawn.clear();
        self.symbols.clear();
    }
    pub(crate) fn symbol(&mut self, at: Pos2, radius: f32, depth: f64) {
        self.symbols.push((at, radius, depth));
    }
    pub(super) fn opaque_at(&self, p: Pos2) -> bool {
        self.depth_at(p).is_finite()
    }
    pub(crate) fn intersects_depth(&self, rect: Rect) -> bool {
        self.drawn
            .iter()
            .any(|key| self.cache[key].0.tile.intersects(rect))
            || self
                .symbols
                .iter()
                .any(|(p, r, _)| rect.distance_sq_to_pos(*p) <= r * r)
    }
    pub(crate) fn depth_at(&self, p: Pos2) -> f64 {
        self.drawn
            .iter()
            .map(|&id| self.body_depth_at(id, p))
            .chain(
                self.symbols
                    .iter()
                    .filter(|(pos, r, _)| pos.distance_sq(p) <= r * r)
                    .map(|(_, _, d)| *d),
            )
            .fold(f64::INFINITY, f64::min)
    }
    pub(super) fn body_depth_at(&self, id: usize, p: Pos2) -> f64 {
        let Some((key, image, ground)) = self.cache.get(&id) else {
            return f64::INFINITY;
        };
        if !key.tile.contains(p) {
            return f64::INFINITY;
        }
        let [w, h] = image.size();
        let x = (((p.x - key.tile.left()) / key.tile.width()) * w as f32) as usize;
        let y = (((p.y - key.tile.top()) / key.tile.height()) * h as f32) as usize;
        ground[y.min(h - 1) * w + x.min(w - 1)]
    }
    fn get(&mut self, name: &'static str, root: &Path) {
        self.files.entry(name).or_insert_with(|| {
            let path = root.with_file_name("textures").join(format!("{name}.jpg"));
            let read = || -> image::ImageResult<RgbImage> {
                let mut input = ImageReader::open(&path)?.with_guessed_format()?;
                let mut bounds = image::Limits::default();
                bounds.max_image_width = Some(8192);
                bounds.max_image_height = Some(8192);
                bounds.max_alloc = Some(256 * 1024 * 1024);
                input.limits(bounds);
                Ok(input.decode()?.into_rgb8())
            };
            read().map_err(|e| format!("{}: {e}", path.display()))
        });
    }
    pub(super) fn draw(
        &mut self,
        painter: &egui::Painter,
        root: &Path,
        camera: SolarCamera,
        scene: &Scene,
        index: usize,
        region: Region,
    ) -> io::Result<bool> {
        let Region {
            viewport: rect,
            tile,
        } = region;
        self.get(TEXTURE_NAMES[index], root);
        if self.air == Air::Multiple && index == 3 && self.emission.is_none() {
            self.emission = Some(
                EmissionMap::load(&root.join("emmap.dat"))
                    .map_err(|e| format!("emmap.dat: {e}（単一散乱で表示）")),
            );
        }
        if index == 3 {
            if self.clouds {
                self.get("earth_cloud", root);
            }
            self.get("earth_lights", root);
        }
        let get = |name: &str| self.files.get(name).and_then(|v| v.as_ref().ok());
        let map = get(TEXTURE_NAMES[index]);
        if map.is_none() && tile != rect {
            return Ok(false);
        }
        let displaced = terrain::near(camera, scene.bodies[index], index);
        let key = Key {
            terrain: displaced.then_some((self.mode, self.preview)),
            camera,
            jd: scene.jd,
            rect,
            tile,
            clouds: self.clouds,
            lights: self.lights,
            eclipse: self.eclipse,
            air: self.air,
            intensity: self.intensity,
        };
        if self.cache.get(&index).is_none_or(|(old, _, _)| old != &key) {
            let sources = Buffers {
                emission: self
                    .emission
                    .as_ref()
                    .and_then(|m| m.as_ref().ok())
                    .filter(|_| index == 3 && self.air == Air::Multiple),
                map,
                cloud: (index == 3 && self.clouds)
                    .then(|| get("earth_cloud"))
                    .flatten(),
                lights: (index == 3).then(|| get("earth_lights")).flatten(),
            };
            let axes = match index {
                0 => [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]],
                10 => mitaka_core::lunar::lunar_orientation(scene.jd)?,
                _ => mitaka_core::rotation::planet_orientation(index - 1, scene.jd)?,
            };
            let ground_source = displaced.then(|| {
                self.terrains
                    .entry(index)
                    .or_insert_with(|| terrain::Layer::open(root, index))
            });
            let (image, ground) = rasterize(&key, scene, index, sources, axes, ground_source)?;
            let tex = painter.ctx().load_texture(
                format!("planet-{index}"),
                image,
                TextureOptions::LINEAR,
            );
            self.cache.insert(index, (key, tex, ground));
        }
        self.drawn.push(index);
        let texture = &self.cache[&index].1;
        painter.image(
            texture.id(),
            tile,
            Rect::from_min_max(Pos2::ZERO, Pos2::new(1., 1.)),
            Color32::WHITE,
        );
        Ok(true)
    }
}
fn rasterize(
    key: &Key,
    scene: &Scene,
    index: usize,
    maps: Buffers<'_>,
    axes: [Point; 3],
    source: Option<&mut terrain::Layer>,
) -> io::Result<(ColorImage, Vec<f64>)> {
    let tile = key.tile;
    let rect = key.rect;
    let camera = key.camera;
    let cap = if key.air != Air::None && matches!(index, 3 | 4) {
        320.
    } else {
        512.
    };
    let sampling = (cap / tile.width().max(tile.height())).min(1.);
    let w = (tile.width() * sampling).ceil().max(1.) as usize;
    let h = (tile.height() * sampling).ceil().max(1.) as usize;
    let mut pixels = vec![Color32::TRANSPARENT; w * h];
    let mut distances = vec![f64::INFINITY; w * h];
    let eclipse = super::eclipse::surface(scene, index, key.eclipse)?;
    let mut atmosphere = match (key.air, index) {
        (Air::None, _) => None,
        (_, 3) => Some(Parameters::earth()),
        (_, 4) => Some(Parameters::mars(RADII_KM[index] * 1000.)?),
        _ => None,
    };
    if let Some(p) = &mut atmosphere {
        p.incident_intensity = key.intensity;
    }
    let radius = RADII_KM[index] / AU_KM;
    let [right, up, outward] = camera.basis();
    let center = scene.bodies[index];
    let displaced = if let Some((source, (mode, preview))) = source.zip(key.terrain) {
        let view = terrain::View::new(
            camera,
            center,
            axes,
            radius,
            [w, h],
            f64::from(rect.width() / rect.height()),
        );
        source.render(&view, mode, preview)
    } else {
        None
    };
    let transform = glam::DMat3::from_cols_array_2d(&axes);
    let eye = scale(
        plus(
            minus(camera.target, center),
            scale(outward, camera.distance),
        ),
        1. / radius,
    );
    let sunlight = if index == 0 {
        [0., 0., 1.]
    } else {
        unit(scale(center, -1.))
    };
    let gain = if key.lights {
        (-dot(unit(eye), sunlight)).max(0.)
    } else {
        (0.3 * (1. - (length(eye) - 1.) / 1.7)).clamp(0., 1.)
    };
    let offset = if index == 0 || index == 4 { 0. } else { 0.5 };
    let focal = f64::from(rect.height()) / (2. * (key.camera.fov * 0.5).to_radians().tan());
    let cloud_scale = 1. + 10. / 6378.1;
    let cloud_eye = scale(eye, 1. / cloud_scale);
    for y in 0..h {
        for x in 0..w {
            let sx = f64::from(tile.left() - rect.center().x)
                + (x as f64 + 0.5) * f64::from(tile.width()) / w as f64;
            let sy = f64::from(rect.center().y - tile.top())
                - (y as f64 + 0.5) * f64::from(tile.height()) / h as f64;
            let ray = unit(minus(
                plus(scale(right, sx / focal), scale(up, sy / focal)),
                outward,
            ));
            let mut rgb = [0.; 3];
            let mut filled = false;
            let ground = match &displaced {
                Some(frame) => {
                    let sample = frame[y * w + x];
                    sample.distance.is_finite().then(|| {
                        (
                            (transform * sample.normal).to_array(),
                            sample.uv.x + offset,
                            sample.uv.y,
                            sample.distance,
                        )
                    })
                }
                None => intersection(eye, ray).map(|n| {
                    let (u, v) = uv(n, axes, offset);
                    (n, u, v, length(minus(eye, n)) * radius)
                }),
            };
            if let Some((normal, u, v, distance)) = ground {
                distances[y * w + x] = distance;
                let cosine = dot(normal, sunlight);
                let light = if index == 0 {
                    1.
                } else {
                    0.3 + 0.7 * cosine.max(0.)
                };
                rgb = scale(
                    maps.map.map_or_else(
                        || {
                            let c = COLORS[index];
                            [c.r(), c.g(), c.b()].map(f64::from)
                        },
                        |m| sample(m, u, v),
                    ),
                    light,
                );
                filled = true;
                if let Some(lights) = maps.lights {
                    rgb = plus(rgb, scale(sample(lights, u, v), gain * (-cosine).max(0.)));
                }
                if let Some(binding) = &eclipse {
                    let footprint = distance * f64::from(tile.height()) / (h as f64 * focal);
                    let point = plus(center, plus(scale(eye, radius), scale(ray, distance)));
                    let effect = binding.effect_at(point, normal, footprint)?;
                    rgb = effect
                        .border_rgb
                        .map(|b| scale(b, 255.))
                        .unwrap_or_else(|| std::array::from_fn(|i| rgb[i] * effect.multiplier[i]));
                }
            }
            if let Some(params) = atmosphere.filter(|_| dot(eye, eye) >= 1.) {
                let eye_m = scale(eye, params.ground_radius_m);
                let ray = Ray {
                    observer_position_m: eye_m,
                    view_direction: ray,
                    sun_direction: sunlight,
                    maximum_distance_m: ground
                        .map(|(_, _, _, distance)| distance / radius * params.ground_radius_m),
                };
                let single = atmosphere::single_scattering(params, ray, 14)?;
                let multiple = match maps
                    .emission
                    .filter(|m| length(eye_m) >= m.ground_radius_m())
                {
                    Some(m) => atmosphere::multiple_scattering(m, ray)?,
                    None => [0.; 3],
                };
                let (color, _) =
                    atmosphere::compose_display(single, multiple, params.incident_intensity)?;
                let add = color.map(|c| f64::from(c.clamp(0., 1.)) * 255.);
                rgb = plus(rgb, add);
                filled |= add.iter().any(|v| *v >= 0.5);
            }
            let cloud_hit = maps.cloud.and_then(|c| {
                intersection(cloud_eye, ray)
                    .filter(|&n| {
                        length(minus(cloud_eye, n)) * radius * cloud_scale < distances[y * w + x]
                    })
                    .map(|n| (c, n))
            });
            if let Some((cloud, n)) = cloud_hit {
                let (u, v) = uv(n, axes, offset);
                let cloud = scale(
                    sample(cloud, u, v),
                    (0.03 + dot(n, sunlight).max(0.)).min(1.),
                );
                let cloud = if let Some(binding) = &eclipse {
                    let effect = binding.effect(n, 0.)?;
                    std::array::from_fn(|i| cloud[i] * effect.multiplier[i])
                } else {
                    cloud
                };
                rgb = plus(rgb, cloud);
                filled |= cloud.iter().any(|v| *v >= 1.);
            }
            if filled {
                let p = y * w + x;
                let [r, g, b] = rgb.map(|v| v.round().clamp(0., 255.) as u8);
                pixels[p] = if ground.is_some() {
                    Color32::from_rgb(r, g, b)
                } else {
                    Color32::from_rgb_additive(r, g, b)
                };
            }
        }
    }
    Ok((
        ColorImage {
            size: [w, h],
            pixels,
        },
        distances,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn disk_shadow_range_and_front_back() {
        let sun = mitaka_core::eclipse::Sphere {
            center_au: [1., 0., 0.],
            radius_au: RADII_KM[0] / AU_KM,
        };
        let visible = |center_au| {
            let occ = mitaka_core::eclipse::Sphere {
                center_au,
                radius_au: 0.01,
            };
            1. - mitaka_core::eclipse::surface_sample(sun, occ, [0.; 3])
                .unwrap()
                .obscured_fraction
        };
        assert_eq!(visible([0.5, 0., 0.]), 0.);
        assert_eq!(visible([2., 0., 0.]), 1.);
        assert_eq!(visible([0.5, 2., 0.]), 1.);
    }
    #[test]
    fn ray_hits_and_antimeridian_texels_blend() {
        assert_eq!(
            intersection([0., 0., 2.], [0., 0., -1.]),
            Some([0., 0., 1.])
        );
        assert_eq!(intersection([0., 0., 2.], [1., 0., 0.]), None);
        let image = RgbImage::from_raw(2, 1, vec![100, 30, 10, 0, 90, 10]).unwrap();
        assert_eq!(sample(&image, 0., 0.5), [50., 60., 10.]);
        assert_eq!(sample(&image, 1., 0.5), [50., 60., 10.]);
    }
}
