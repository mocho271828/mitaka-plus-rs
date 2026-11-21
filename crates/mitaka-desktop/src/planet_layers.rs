//! sky_model.cpp と radius_display.cpp の地心視位置と模式半径。
use super::{
    chart::Chart,
    projection::View,
    solar::{COLORS, NAMES},
};
use eframe::egui::{self, Color32, FontId, Pos2};
use mitaka_core::{
    astronomy::equatorial_coordinates,
    ephemeris::Cf96Ephemeris,
    lunar::{earth_center, lunar_position},
};
use std::io;
pub(crate) struct PlanetLayers {
    pub visible: bool,
    pub labels: bool,
    pub axes: bool,
    pub scales: [f32; 4],
    pub sky_selected: Option<usize>,
    pub horizon_selected: Option<usize>,
}
impl Default for PlanetLayers {
    fn default() -> Self {
        Self {
            visible: true,
            labels: true,
            axes: false,
            scales: [1.; 4],
            sky_selected: None,
            horizon_selected: None,
        }
    }
}
pub(crate) struct SkyBody {
    pub id: usize,
    pub equatorial: [f64; 2],
    pub distance_au: f64,
}
pub(crate) fn positions(ephemeris: &Cf96Ephemeris, jd: f64) -> io::Result<Vec<SkyBody>> {
    let moon = lunar_position(jd)?;
    let earth = earth_center(ephemeris.state(2, jd)?.position_au, moon);
    (0..11)
        .filter(|id| *id != 3)
        .map(|id| {
            let v = match id {
                0 => earth.map(|x| -x),
                10 => moon,
                i => {
                    let p = ephemeris.state(i - 1, jd)?.position_au;
                    std::array::from_fn(|a| p[a] - earth[a])
                }
            };
            Ok(SkyBody {
                id,
                equatorial: equatorial_coordinates(v)?,
                distance_au: v[0].hypot(v[1]).hypot(v[2]),
            })
        })
        .collect()
}
impl PlanetLayers {
    pub fn moon_orbit_scale(&self) -> f64 {
        if self.scales[0] == 1. {
            1.
        } else {
            f64::from(self.scales[0] / 20.)
        }
    }
    pub fn selected(&self, view: View) -> Option<usize> {
        match view {
            View::Sky => self.sky_selected,
            View::Horizon => self.horizon_selected,
            _ => None,
        }
    }
    pub fn select(&mut self, view: View, id: Option<usize>) {
        match view {
            View::Sky => self.sky_selected = id,
            View::Horizon => self.horizon_selected = id,
            _ => {}
        }
    }
    pub fn panel(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.visible, "惑星本体");
            ui.checkbox(&mut self.labels, "惑星名");
            ui.checkbox(&mut self.axes, "自転軸");
        });
        ui.collapsing("天体の表示倍率", |ui| {
            for (name, scale) in [
                "宇宙の惑星・月",
                "宇宙の太陽",
                "星図の惑星",
                "星図の太陽・月",
            ]
            .into_iter()
            .zip(&mut self.scales)
            {
                ui.horizontal(|ui| {
                    ui.label(name);
                    ui.add(
                        egui::DragValue::new(scale)
                            .speed(0.1)
                            .range(0.001..=1000000.),
                    );
                });
            }
        });
    }
    pub fn paint(
        &self,
        chart: &Chart<'_>,
        painter: &egui::Painter,
        bodies: &[SkyBody],
        click: Option<Pos2>,
    ) -> Option<usize> {
        if !self.visible && !self.labels {
            return None;
        }
        let mut closest = f32::INFINITY;
        let mut picked = None;
        for body in bodies {
            let [ra, dec] = body.equatorial;
            let Some(coords) = chart.observed(ra as f32, dec as f32) else {
                continue;
            };
            let p = chart.point(coords);
            let id = body.id;
            let r = (4. * self.scales[if id == 0 || id == 10 { 3 } else { 2 }]).clamp(1., 120.);
            if !chart.rect.expand(r).contains(p) {
                continue;
            }
            if self.visible {
                painter.circle_filled(p, r, COLORS[id]);
            }
            if self.labels {
                painter.text(
                    p + egui::vec2(r + 4., 0.),
                    egui::Align2::LEFT_CENTER,
                    NAMES[id],
                    FontId::proportional(12.),
                    COLORS[id],
                );
            }
            if self.selected(chart.view) == Some(id) {
                painter.circle_stroke(p, (r + 4.).max(9.), egui::Stroke::new(1_f32, Color32::GOLD));
            }
            if let Some(d) = click
                .map(|q| p.distance(q))
                .filter(|d| *d <= r.max(8.) && *d < closest)
            {
                closest = d;
                picked = Some(id);
            }
        }
        picked
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "実カタログの地心位置"]
    fn geocentric_j2000() {
        let root = std::path::PathBuf::from(std::env::var_os("MITAKA_DATA_ROOT").unwrap());
        let eph = Cf96Ephemeris::load(&root.join("cf96.dat")).unwrap();
        let bodies = positions(&eph, 2451545.).unwrap();
        assert_eq!(bodies.len(), 10);
        let sun = &bodies[0];
        assert!(
            (sun.equatorial[0] - 281.29).abs() < 0.03 && (sun.equatorial[1] + 23.03).abs() < 0.03
        );
        let moon = bodies.last().unwrap();
        assert!((0.0024..0.0028).contains(&moon.distance_au));
        assert!(bodies.iter().all(|b| b.id != 3));
    }
}
