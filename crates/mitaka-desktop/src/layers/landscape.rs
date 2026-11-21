//! 地平図の観測地点に紐づくパノラマ。Mitaka_plus f9fe624 の
//! landscape_selection.cpp、landscape_projection.cpp、HorizonLandscapeRenderer.mm を参照。
use crate::{media_preview::image::load_landscape, projection::Camera};
use eframe::egui::{self, Color32, ColorImage, Painter, Pos2, Rect, TextureHandle};
use image::RgbaImage;
use mitaka_core::place_catalog::{Landscape, PlaceCatalog};
use std::{
    collections::HashMap,
    io,
    path::{Path, PathBuf},
    sync::Arc,
};

fn invalid(reason: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, reason)
}
#[derive(Clone, Copy, PartialEq)]
struct Mapping {
    scale: f64,
    vertical_offset: f64,
    rotation: f64,
}
impl Mapping {
    fn new(d: &Landscape) -> io::Result<Self> {
        let map = Self {
            scale: d.scale,
            vertical_offset: d.offsets[0],
            rotation: d.offsets[1].rem_euclid(360.),
        };
        if !map.scale.is_finite()
            || map.scale <= 0.
            || !map.rotation.is_finite()
            || !map.vertical_offset.is_finite()
            || [-90., 90.]
                .iter()
                .any(|&alt| !map.uv(0., alt)[1].is_finite())
        {
            return Err(invalid("風景の倍率またはオフセットが不正です"));
        }
        Ok(map)
    }
    fn uv(self, az: f64, alt: f64) -> [f64; 2] {
        [
            (az.rem_euclid(360.) + 180. + self.rotation).rem_euclid(360.) / 360.,
            0.5 - alt / 180. / self.scale + self.vertical_offset,
        ]
    }
}
#[derive(Default)]
pub struct Landscapes {
    images: HashMap<usize, (Mapping, Arc<RgbaImage>)>,
}
fn resolve(root: &Path, raw: &[u8]) -> io::Result<PathBuf> {
    let path = match std::str::from_utf8(raw) {
        Ok(path) => path.into(),
        Err(_) => encoding_rs::SHIFT_JIS
            .decode_without_bom_handling_and_without_replacement(raw)
            .ok_or_else(|| invalid("風景パスの文字コードが不正です"))?,
    };
    let path = path.replace('\\', "/");
    if path.is_empty() || path.starts_with('/') || path.contains([':', '\0']) {
        return Err(invalid("風景画像は相対パスで指定してください"));
    }
    let path = root.join(path).canonicalize()?;
    if !path.starts_with(root) || path == root {
        return Err(invalid("風景画像がデータルートの外部にあります"));
    }
    Ok(path)
}
impl Landscapes {
    pub fn load(data: &Path, catalog: &PlaceCatalog, warnings: &mut Vec<String>) -> Self {
        let mut out = Self::default();
        let mut cache: HashMap<PathBuf, Arc<RgbaImage>> = HashMap::new();
        let root = data.parent().unwrap_or(data).canonicalize();
        let mut total = 0;
        for (i, d) in catalog.landscapes.iter().enumerate() {
            if !(d.body == b"Planet.Earth" || (d.body == b"Planet" && d.name == b"Default"))
                || d.path.is_empty()
            {
                continue;
            }
            let read = (|| -> io::Result<_> {
                let map = Mapping::new(d)?;
                let path = resolve(
                    root.as_ref()
                        .map_err(|e| io::Error::new(e.kind(), e.to_string()))?,
                    &d.path,
                )?;
                let tex = if let Some(tex) = cache.get(&path) {
                    tex.clone()
                } else {
                    let tex = Arc::new(load_landscape(&path)?);
                    if tex.as_raw().len() > 128 * 1024 * 1024 - total {
                        return Err(invalid("風景画像キャッシュは128 MiBまでです"));
                    }
                    total += tex.as_raw().len();
                    cache.insert(path, tex.clone());
                    tex
                };
                Ok((map, tex))
            })();
            match read {
                Ok(texture) => {
                    out.images.insert(i, texture);
                }
                Err(e) => warnings.push(format!("{}: {e}", String::from_utf8_lossy(&d.path))),
            }
        }
        out
    }
    fn selection<'a>(
        &'a self,
        catalog: &PlaceCatalog,
        identifier: &[u8],
    ) -> Option<&'a (Mapping, Arc<RgbaImage>)> {
        let find = |body: &[u8], name: &[u8]| {
            catalog
                .landscapes
                .iter()
                .rposition(|d| d.body == body && d.name == name)
        };
        let place = catalog
            .places
            .iter()
            .find(|p| p.id == identifier && p.earth());
        let idx = place
            .and_then(|p| find(b"Planet.Earth", &p.landscape))
            .or_else(|| find(b"Planet.Earth", b"Default"))
            .or_else(|| find(b"Planet", b"Default"))?;
        self.images.get(&idx)
    }
}
fn sample(image: &RgbaImage, [u, v]: [f64; 2]) -> [u8; 4] {
    let (w, h) = image.dimensions();
    let x = (u * f64::from(w) - 0.5).clamp(0., f64::from(w - 1));
    let y = (v * f64::from(h) - 0.5).clamp(0., f64::from(h - 1));
    let (left, top) = (x as u32, y as u32);
    let (right, bottom) = ((left + 1).min(w - 1), (top + 1).min(h - 1));
    let (dx, dy) = (x - f64::from(left), y - f64::from(top));
    let points = [
        image.get_pixel(left, top),
        image.get_pixel(right, top),
        image.get_pixel(left, bottom),
        image.get_pixel(right, bottom),
    ];
    let weights = [
        (1. - dx) * (1. - dy),
        dx * (1. - dy),
        (1. - dx) * dy,
        dx * dy,
    ];
    // 原画像のRGBAを個別に補間してから、eguiのpremultiplied alphaへ変換する。
    std::array::from_fn(|i| {
        points
            .iter()
            .zip(weights)
            .map(|(p, w)| f64::from(p[i]) * w)
            .sum::<f64>()
            .round() as u8
    })
}
fn raster(
    texture: &RgbaImage,
    map: Mapping,
    (camera, roll): (Camera, f64),
    [w, h]: [usize; 2],
) -> ColorImage {
    let columns: Vec<_> = (0..w)
        .map(|x| {
            (((x as f64 + 0.5) / w as f64 - 0.5) * 360. / camera.zoom)
                .to_radians()
                .sin_cos()
        })
        .collect();
    let (st, ct) = camera.latitude.to_radians().sin_cos();
    let (sr, cr) = roll.to_radians().sin_cos();
    let mut pixels = Vec::with_capacity(w * h);
    for y in 0..h {
        let (up, plane) = ((0.5 - (y as f64 + 0.5) / h as f64) * 180. / camera.zoom)
            .to_radians()
            .sin_cos();
        for &(s, c) in &columns {
            let (forward, projected_right) = (plane * c, plane * s);
            let (right, up) = (
                projected_right * cr - up * sr,
                up * cr + projected_right * sr,
            );
            let north = forward * ct - up * st;
            let alt = (forward * st + up * ct)
                .atan2(right.hypot(north))
                .to_degrees();
            let az = camera.longitude + right.atan2(north).to_degrees();
            let [r, g, b, a] = sample(texture, map.uv(az, alt));
            pixels.push(Color32::from_rgba_unmultiplied(r, g, b, a));
        }
    }
    ColorImage {
        size: [w, h],
        pixels,
    }
}
struct Cached {
    image: Arc<RgbaImage>,
    map: Mapping,
    view: [f64; 4],
    pixels: ColorImage,
    texture: TextureHandle,
}
#[derive(Default)]
pub struct LandscapeRenderer {
    cached: Option<Cached>,
    rect: Option<Rect>,
}
impl LandscapeRenderer {
    pub fn prepare(
        &mut self,
        painter: &Painter,
        d: &Landscapes,
        catalog: &PlaceCatalog,
        identifier: &[u8],
        (camera, roll): (Camera, f64),
        rect: Rect,
    ) {
        self.rect = Some(rect);
        let Some((map, image)) = d.selection(catalog, identifier) else {
            self.cached = None;
            return;
        };
        let largest = rect.width().max(rect.height());
        if !largest.is_finite() || largest <= 0. {
            self.cached = None;
            return;
        }
        let factor = (1024. / largest).min(1.);
        let size =
            [rect.width(), rect.height()].map(|v| (v * factor).ceil().clamp(1., 1024.) as usize);
        let view = [camera.longitude, camera.latitude, camera.zoom, roll];
        if self.cached.as_ref().is_some_and(|c| {
            c.view == view && c.map == *map && Arc::ptr_eq(&c.image, image) && c.pixels.size == size
        }) {
            return;
        }
        let pixels = raster(image, *map, (camera, roll), size);
        let texture = painter.ctx().load_texture(
            "観測地点の風景",
            pixels.clone(),
            egui::TextureOptions::LINEAR,
        );
        self.cached = Some(Cached {
            image: image.clone(),
            map: *map,
            view,
            pixels,
            texture,
        });
    }
    pub fn opaque_at(&self, point: Pos2) -> bool {
        let (Some(c), Some(rect)) = (&self.cached, self.rect) else {
            return false;
        };
        if !rect.contains(point) {
            return false;
        }
        let [w, h] = c.pixels.size;
        let x = (((point.x - rect.left()) / rect.width()) * w as f32 - 0.5).floor() as isize;
        let y = (((point.y - rect.top()) / rect.height()) * h as f32 - 0.5).floor() as isize;
        (-1..=2).all(|dy| {
            (-1..=2).all(|dx| {
                let i = (x + dx).clamp(0, w as isize - 1) as usize;
                let j = (y + dy).clamp(0, h as isize - 1) as usize;
                c.pixels.pixels[j * w + i].a() == 255
            })
        })
    }
    pub fn paint(&self, painter: &Painter, rect: Rect) {
        if let Some(c) = &self.cached {
            painter.image(
                c.texture.id(),
                rect,
                Rect::from_min_max(Pos2::ZERO, egui::pos2(1., 1.)),
                Color32::WHITE,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mitaka_core::{catalog::CatalogLimits, place_catalog};
    #[test]
    fn straight_alpha_filter_and_uv_edges() {
        let texture = RgbaImage::from_raw(2, 1, vec![255, 0, 0, 0, 0, 0, 255, 255]).unwrap();
        assert_eq!(sample(&texture, [0.5, 0.5]), [128, 0, 128, 128]);
        assert_eq!(sample(&texture, [2., -2.]), [0, 0, 255, 255]);
        let definition = Landscape {
            body: b"Planet.Earth".to_vec(),
            name: b"Sky".to_vec(),
            path: vec![],
            scale: 0.25,
            offsets: [0.1, 90.],
        };
        let map = Mapping::new(&definition).unwrap();
        assert_eq!(map.uv(90., 0.), [0., 0.6]);
        let pix = raster(
            &texture,
            map,
            (
                Camera {
                    longitude: 180.,
                    latitude: 0.,
                    zoom: 2.,
                    roll: 0.,
                },
                0.,
            ),
            [1, 1],
        );
        assert_eq!(pix.pixels[0], Color32::from_rgba_unmultiplied(255, 0, 0, 0));
    }
    #[test]
    fn resolves_only_images_in_selected_tree_and_blocks_opaque_clicks() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("set");
        let data = root.join("data");
        std::fs::create_dir_all(&data).unwrap();
        let image = RgbaImage::from_raw(1, 1, vec![255; 4]).unwrap();
        image.save(root.join("image.png")).unwrap();
        image.save(temp.path().join("outside.png")).unwrap();
        assert!(resolve(&root.canonicalize().unwrap(), b"../outside.png").is_err());
        let records = place_catalog::parse(
            br#"Places:addLandscape("Planet","Default","../outside.png",1,0,0);
Places:addLandscape("Planet.Earth","Default","image.png",1,0,0);
Places:addLandscape("Planet.Earth","Local","image.png",1,0,0);
Places:addPlace("Planet.Earth.Site","Site",10,20,"","Local");"#,
            CatalogLimits::default(),
        )
        .unwrap();
        let mut warnings = Vec::new();
        let loaded = Landscapes::load(&data, &records, &mut warnings);
        assert_eq!(warnings.len(), 1);
        assert_eq!(loaded.images.len(), 2);
        assert!(Arc::ptr_eq(&loaded.images[&1].1, &loaded.images[&2].1));
        assert!(loaded.selection(&records, b"Planet.Earth.Site").is_some());
        let ctx = egui::Context::default();
        let mut renderer = LandscapeRenderer::default();
        let _ = ctx.run(Default::default(), |ctx| {
            let rect = Rect::from_min_size(Pos2::ZERO, egui::vec2(6., 4.));
            let painter = ctx.debug_painter();
            renderer.prepare(
                &painter,
                &loaded,
                &records,
                b"Planet.Earth.Site",
                (Camera::default(), 0.),
                rect,
            );
            assert!(renderer.opaque_at(rect.center()));
            assert!(!renderer.opaque_at(egui::pos2(-1., 1.)));
            renderer.paint(&painter, rect);
        });
    }
    #[test]
    #[ignore = "requires Mitaka Plus data"]
    fn plus_registered_panorama_has_transparent_sky_and_opaque_land() {
        let root = PathBuf::from(std::env::var_os("MITAKA_DATA_ROOT").unwrap());
        let records = place_catalog::load_observer_places(&root).unwrap();
        let mut warnings = Vec::new();
        let loaded = Landscapes::load(&root, &records, &mut warnings);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(loaded.images.len(), 2);
        let (map, t) = loaded.selection(&records, b"Planet.Earth.Tatesho").unwrap();
        let pix = raster(t, *map, (Camera::default(), 0.), [144, 72]);
        assert!(pix.pixels.iter().any(|p| p.a() == 0));
        assert!(pix.pixels.iter().any(|p| p.a() == 255));
        assert!(loaded.selection(&records, b"unregistered").is_none());
    }
}
