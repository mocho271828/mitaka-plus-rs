//! 地形ファイルに格納された格子を、天体固定座標の局所表示に使う。
//! 地点標高は最高段の双線形補間、着陸高度は描画段の三角形交点から求める。
use eframe::egui::{self, Align2, Color32, FontId, Pos2, Stroke};
use mitaka_core::{
    ephemeris::Cf96Ephemeris,
    lunar::lunar_orientation,
    rotation::planet_orientation,
    terrain::{unit_direction, Amplification, Terrain},
};
use std::{io, path::Path};

type Vector = [f64; 3];
fn dot(a: Vector, b: Vector) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
struct LocalVertex {
    point: Vector,
    normal: Vector,
    color: [u8; 3],
}
struct Relief {
    vertices: Vec<LocalVertex>,
    faces: Vec<[u32; 3]>,
    landing_ratio: f64,
    tiles: usize,
}
pub struct Surface {
    pub body: usize,
    pub lat: f64,
    pub lon: f64,
    pub central_height: f64,
    pub azimuth: f64,
    pub span_km: f64,
    data: Terrain,
    relief: Relief,
    pub mode: Amplification,
    texture: Option<image::RgbaImage>,
    warning: Option<String>,
}
/// f9fe624 terrain_snapshot.cpp のデータディレクトリ相対パス。
pub(crate) fn terrain_file(body: usize) -> Option<&'static str> {
    match body {
        1 => Some("mercury_topo.dat"),
        3 => Some("earth_topo/earth_topo.dat"),
        4 => Some("mars_topo/mars_topo.dat"),
        10 => Some("moon_topo.dat"),
        _ => None,
    }
}

impl Surface {
    pub(crate) fn load_optional(
        root: &Path,
        body: usize,
        lat: f64,
        lon: f64,
        mode: Amplification,
    ) -> io::Result<Option<Self>> {
        if terrain_file(body).is_none() {
            return Ok(None);
        }
        match Self::load_mode(root, body, lat, lon, 100., mode) {
            Ok(mesh) => Ok(Some(mesh)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }

    pub fn load(root: &Path, body: usize, lat: f64, lon: f64, span_km: f64) -> io::Result<Self> {
        Self::load_mode(root, body, lat, lon, span_km, Amplification::Actual)
    }
    pub fn load_mode(
        root: &Path,
        body: usize,
        lat: f64,
        lon: f64,
        span_km: f64,
        mode: Amplification,
    ) -> io::Result<Self> {
        let file = terrain_file(body)
            .ok_or_else(|| io::Error::other("この天体の地形データには対応していません"))?;
        let name = crate::solar::sphere::TEXTURE_NAMES[body];
        if !span_km.is_finite() || !(0.1..=1000.).contains(&span_km) {
            return Err(io::Error::other(
                "地表表示範囲は0.1〜1000 kmで指定してください",
            ));
        }
        let mut data = Terrain::open(&root.join(file))?;
        let central_height = data.elevation_m(lon, lat)?;
        let tex_path = root
            .parent()
            .unwrap_or(root)
            .join(format!("textures/{name}.jpg"));
        let (texture, warning) = match crate::media_preview::image::load_landscape(&tex_path) {
            Ok(img) => (Some(img), None),
            Err(error) => (None, Some(format!("{}: {error}", tex_path.display()))),
        };
        let relief = Relief::build(&mut data, body, lat, lon, span_km, mode, texture.as_ref())?;
        Ok(Self {
            body,
            lat,
            lon,
            central_height,
            azimuth: 0.55,
            span_km,
            data,
            relief,
            mode,
            texture,
            warning,
        })
    }
    pub(crate) fn relocate(&mut self, lat: f64, lon: f64) -> io::Result<()> {
        if self.lat == lat && self.lon == lon {
            return Ok(());
        }
        let height = self.data.elevation_m(lon, lat)?;
        let relief = Relief::build(
            &mut self.data,
            self.body,
            lat,
            lon,
            self.span_km,
            self.mode,
            self.texture.as_ref(),
        )?;
        self.lat = lat;
        self.lon = lon;
        self.central_height = height;
        self.relief = relief;
        Ok(())
    }
    pub fn set_mode(&mut self, mode: Amplification) -> io::Result<()> {
        if self.mode != mode {
            let relief = Relief::build(
                &mut self.data,
                self.body,
                self.lat,
                self.lon,
                self.span_km,
                mode,
                self.texture.as_ref(),
            )?;
            self.relief = relief;
            self.mode = mode;
        }
        Ok(())
    }
    /// 地点標高とは別に、実際の表示基準半径から地形交点までの差（メートル）を返す。
    pub fn landing_height_m(&self, display_radius_km: f64) -> f64 {
        (self.relief.landing_ratio - 1.) * display_radius_km * 1000.
    }
    fn sun(&self, eph: &Cf96Ephemeris, jd: f64) -> io::Result<Vector> {
        let c = crate::solar::surface_center(eph, self.body, jd)?;
        let axes = if self.body == 10 {
            lunar_orientation(jd)?
        } else {
            planet_orientation(self.body - 1, jd)?
        };
        let n = c[0].hypot(c[1]).hypot(c[2]);
        let sun = c.map(|v| -v / n);
        Ok(axes.map(|axis| dot(sun, axis)))
    }
    pub fn draw(&mut self, ui: &mut egui::Ui, eph: &Cf96Ephemeris, jd: f64) {
        let (res, painter) = ui.allocate_painter(ui.available_size(), egui::Sense::drag());
        let rect = res.rect;
        if res.dragged() {
            self.azimuth += f64::from(res.drag_delta().x) * 0.008;
        }
        let sun = match self.sun(eph, jd) {
            Ok(sun) => sun,
            Err(e) => {
                painter.text(
                    rect.center(),
                    Align2::CENTER_CENTER,
                    e.to_string(),
                    FontId::proportional(14.),
                    Color32::WHITE,
                );
                return;
            }
        };
        let (s, c) = self.azimuth.sin_cos();
        let km_per_radius = self.data.radius_m() / 1000.;
        let scale =
            f64::from(rect.width().min(rect.height())) * 0.75 * km_per_radius / self.span_km;
        let centers = [rect.center().x, rect.center().y];
        let mut mesh = egui::Mesh::default();
        let mut depths = Vec::with_capacity(self.relief.vertices.len());
        for v in &self.relief.vertices {
            let [x, y, z] = v.point;
            let right = c * x + s * y;
            let forward = -s * x + c * y;
            let up = z - self.relief.landing_ratio;
            let screen = Pos2::new(
                centers[0] + (right * scale) as f32,
                centers[1] - ((forward * 0.5 + up * 0.8660254037844386) * scale) as f32,
            );
            let light = 0.12 + 0.88 * dot(v.normal, sun).clamp(0., 1.);
            let [r, g, b] = v.color.map(|v| (f64::from(v) * light).round() as u8);
            mesh.colored_vertex(screen, Color32::from_rgb(r, g, b));
            depths.push(-forward * 0.8660254037844386 + up * 0.5);
        }
        let mut faces = self
            .relief
            .faces
            .iter()
            .map(|f| (f.iter().map(|&j| depths[j as usize]).sum::<f64>(), *f))
            .collect::<Vec<_>>();
        faces.sort_by(|a, b| a.0.total_cmp(&b.0));
        for (_, face) in faces {
            mesh.indices.extend(face);
        }
        painter.add(egui::Shape::mesh(mesh));
        painter.circle_stroke(rect.center(), 5., Stroke::new(1.5_f32, Color32::WHITE));
        let text=format!("{}  緯度 {:+.4}°  東経 {:+.4}°\n地点標高 {:.1} m  起伏 ×{}  表示範囲 {:.0} km  {} タイル",
            super::solar::NAMES[self.body],self.lat,self.lon,self.central_height,self.mode.factor(),self.span_km,self.relief.tiles);
        painter.text(
            Pos2::new(rect.left() + 14., rect.bottom() - 14.),
            Align2::LEFT_BOTTOM,
            text,
            FontId::proportional(14.),
            Color32::WHITE,
        );
        if let Some(message) = &self.warning {
            painter.text(
                rect.left_top() + egui::vec2(8., 8.),
                Align2::LEFT_TOP,
                message,
                FontId::proportional(12.),
                Color32::LIGHT_GRAY,
            );
        }
    }
}
impl Relief {
    fn build(
        data: &mut Terrain,
        body: usize,
        lat: f64,
        lon: f64,
        span: f64,
        mode: Amplification,
        texture: Option<&image::RgbaImage>,
    ) -> io::Result<Self> {
        let up = unit_direction(lon, lat);
        let east = unit_direction(lon + 90., 0.);
        let north = unit_direction(lon, lat + 90.);
        let extent = span * 1000. / data.radius_m() / 2.;
        let mut out = Self {
            vertices: vec![],
            faces: vec![],
            landing_ratio: 0.,
            tiles: 0,
        };
        for patch in 0..data.patch_count() {
            let bounds = data.bounds(patch)?;
            if !bounds.near(up, extent * std::f64::consts::SQRT_2) {
                continue;
            }
            let level = bounds.level(up, false)?;
            let tile = data.grid(patch, level)?.mesh(mode)?;
            if bounds.near(up, 0.) {
                if let Some(radius) = tile.radial_intersection(up) {
                    out.landing_ratio = out.landing_ratio.max(radius);
                }
            }
            let mut local = vec![[0.; 3]; tile.vertices.len()];
            for (p, v) in local.iter_mut().zip(&tile.vertices) {
                *p = [
                    dot(v.position, east),
                    dot(v.position, north),
                    dot(v.position, up),
                ];
            }
            let mut indices = vec![None; tile.vertices.len()];
            let before = out.faces.len();
            for tri in tile.triangles() {
                let coords = tri.map(|i| local[i]);
                if coords.iter().all(|p| p[2] <= 0.)
                    || (0..2).any(|axis| {
                        coords.iter().all(|p| p[axis] < -extent)
                            || coords.iter().all(|p| p[axis] > extent)
                    })
                {
                    continue;
                }
                if out.faces.len() >= 500_000 {
                    return Err(io::Error::other("局所地形の三角形数が上限を超えました"));
                }
                let face = tri.map(|i| {
                    *indices[i].get_or_insert_with(|| {
                        let v = &tile.vertices[i];
                        let color = if let Some(t) = texture {
                            sample(t, v.uv, body)
                        } else if body == 3 {
                            [100, 141, 107]
                        } else if body == 4 {
                            [175, 112, 84]
                        } else {
                            [150, 150, 150]
                        };
                        let id = out.vertices.len() as u32;
                        out.vertices.push(LocalVertex {
                            point: local[i],
                            normal: v.normal,
                            color,
                        });
                        id
                    })
                });
                out.faces.push(face);
            }
            if before != out.faces.len() {
                out.tiles += 1;
            }
        }
        data.verify_unchanged()?;
        if out.faces.is_empty() || out.landing_ratio <= 0. {
            return Err(io::Error::other("指定地点の地形交点がありません"));
        }
        Ok(out)
    }
}
fn sample(image: &image::RgbaImage, uv: [f64; 2], body: usize) -> [u8; 3] {
    let (w, h) = image.dimensions();
    let u = uv[0] + if body == 4 { 0. } else { 0.5 };
    let x = u.rem_euclid(1.) * f64::from(w) - 0.5;
    let y = (uv[1] * f64::from(h) - 0.5).clamp(0., f64::from(h - 1));
    let a = (x.floor() as i64).rem_euclid(i64::from(w)) as u32;
    let b = (a + 1) % w;
    let t = y.floor() as u32;
    let d = (t + 1).min(h - 1);
    let (fx, fy) = (x - x.floor(), y - y.floor());
    std::array::from_fn(|i| {
        let col = |x, y| f64::from(image.get_pixel(x, y)[i]);
        ((col(a, t) * (1. - fx) + col(b, t) * fx) * (1. - fy)
            + (col(a, d) * (1. - fx) + col(b, d) * fx) * fy)
            .round() as u8
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires Mitaka Plus Earth and Mars LFS terrain"]
    fn landing_uses_original_lod_triangles_and_all_five_amplifications() {
        let root = std::path::PathBuf::from(std::env::var_os("MITAKA_DATA_ROOT").unwrap());
        for (body, lat, lon, expected) in [
            (
                3,
                35.68,
                139.76,
                [
                    8.776824677,
                    17.576960274,
                    43.977356228,
                    87.977980028,
                    175.979092158,
                ],
            ),
            (
                4,
                -4.59,
                137.44,
                [
                    -4503.262080623,
                    -9006.433881036,
                    -22515.949309107,
                    -45031.808446427,
                    -90063.527068906,
                ],
            ),
        ] {
            let mut s = Surface::load(&root, body, lat, lon, 100.).unwrap();
            let raw = s.central_height;
            assert!(s.warning.is_none());
            for (mode, height) in Amplification::ALL.into_iter().zip(expected) {
                s.set_mode(mode).unwrap();
                assert_eq!(raw, s.central_height);
                let actual = s.landing_height_m(super::super::solar::RADII_KM[body]);
                assert!(
                    (actual - height).abs() < 2e-5,
                    "{body} {mode:?} {actual} vs {height}"
                );
                assert!(s.relief.faces.len() > 300 && s.relief.tiles > 0);
            }
        }
    }
}
