//! 地形ファイルの双線形補間値を使って、観測地点の周囲を局所座標で表示する。
use eframe::egui::{self, Align2, Color32, FontId, Pos2, Stroke};
use mitaka_core::terrain::Terrain;
use std::{io, path::Path};
const GRID: usize = 25;
pub struct Surface {
    pub body: usize,
    pub lat: f64,
    pub lon: f64,
    radius: f64,
    samples: Vec<f64>,
    pub central_height: f64,
    pub azimuth: f64,
    pub span_km: f64,
}
impl Surface {
    pub fn load(root: &Path, body: usize, lat: f64, lon: f64, span_km: f64) -> io::Result<Self> {
        let file = match body {
            3 => "earth",
            4 => "mars",
            _ => return Err(io::Error::other("地形データは地球と火星に対応します")),
        };
        let mut terrain = Terrain::open(&root.join(format!("{file}_topo/{file}_topo.dat")))?;
        let radius = terrain.radius_m();
        let central_height = terrain.elevation_m(lon, lat)?;
        let mut samples = Vec::with_capacity(GRID * GRID);
        let (sl, cl) = lat.to_radians().sin_cos();
        for row in 0..GRID {
            for col in 0..GRID {
                let east = (col as f64 / (GRID - 1) as f64 - 0.5) * 1000. * span_km;
                let north = (0.5 - row as f64 / (GRID - 1) as f64) * 1000. * span_km;
                let distance = east.hypot(north) / radius;
                let bearing = east.atan2(north);
                let (sd, cd) = distance.sin_cos();
                let latitude = (sl * cd + cl * sd * bearing.cos()).clamp(-1., 1.).asin();
                let longitude = lon
                    + (bearing.sin() * sd * cl)
                        .atan2(cd - sl * latitude.sin())
                        .to_degrees();
                samples.push(terrain.elevation_m(
                    (longitude + 180.).rem_euclid(360.) - 180.,
                    latitude.to_degrees(),
                )?);
            }
        }
        Ok(Self {
            body,
            lat,
            lon,
            radius,
            samples,
            central_height,
            azimuth: 0.55,
            span_km,
        })
    }
    pub fn draw(&mut self, ui: &mut egui::Ui) {
        let (res, painter) = ui.allocate_painter(ui.available_size(), egui::Sense::drag());
        let rect = res.rect;
        if res.dragged() {
            self.azimuth += f64::from(res.drag_delta().x) * 0.008;
        }
        let (s, c) = self.azimuth.sin_cos();
        let scale = f64::from(rect.width().min(rect.height())) * 0.64;
        let (mut faces, mut center) = (Vec::with_capacity((GRID - 1) * (GRID - 1)), rect.center());
        let mut points = Vec::with_capacity(self.samples.len());
        for (i, &height) in self.samples.iter().enumerate() {
            let x = (i % GRID) as f64 / (GRID - 1) as f64 - 0.5;
            let y = (i / GRID) as f64 / (GRID - 1) as f64 - 0.5;
            let right = c * x + s * y;
            let depth = -s * x + c * y;
            let z = (height - self.central_height) / (self.span_km * 1000.);
            let p = rect.center()
                + egui::vec2(
                    (right * scale) as f32,
                    ((depth * 0.5 - z * 0.866) * scale) as f32,
                );
            points.push((p, depth));
            if i == self.samples.len() / 2 {
                center = p;
            }
        }
        for row in 0..GRID - 1 {
            for col in 0..GRID - 1 {
                let a = row * GRID + col;
                let indices = [a, a + 1, a + GRID + 1, a + GRID];
                let depth = indices.iter().map(|&i| points[i].1).sum::<f64>();
                faces.push((depth, indices));
            }
        }
        faces.sort_by(|a, b| a.0.total_cmp(&b.0));
        for (_, indices) in faces {
            let h = indices.iter().map(|&i| self.samples[i]).sum::<f64>() / 4.;
            let tone = ((h - self.central_height) / 2000.).clamp(-1., 1.);
            let base = if self.body == 3 {
                [60., 103., 87.]
            } else {
                [160., 87., 58.]
            };
            let channel = |v: f64| (v + tone * 42.).clamp(0., 255.) as u8;
            let color = Color32::from_rgb(channel(base[0]), channel(base[1]), channel(base[2]));
            painter.add(egui::Shape::convex_polygon(
                indices.map(|i| points[i].0).to_vec(),
                color,
                Stroke::new(0.45_f32, Color32::from_black_alpha(48)),
            ));
        }
        painter.circle_stroke(center, 5., Stroke::new(1.5_f32, Color32::WHITE));
        painter.text(Pos2::new(rect.left()+14.,rect.bottom()-14.),Align2::LEFT_BOTTOM,format!("{}  緯度 {:+.4}°  東経 {:+.4}°\n地点標高 {:.1} m  中心距離 {:.3} km  地形範囲 {:.0} km",super::solar::NAMES[self.body],self.lat,self.lon,self.central_height,(self.radius+self.central_height)/1000.,self.span_km),FontId::proportional(14.),Color32::WHITE);
    }
}
