//! Mitaka_plus f9fe624 horizon_objects.cpp / HorizonView.mm の地平図上の小天体・探査機。
use super::{catalog::Catalog, Browser, Kind, Position};
use crate::{catalog::SkyData, chart::Chart, viewer::Picking};
use eframe::egui::{self, Align2, Color32, FontId, Painter, Pos2, Stroke};
use mitaka_core::topocentric::{Apparent, Observer};
use std::{cell::OnceCell, rc::Rc};

struct Cache {
    catalog: Rc<Catalog>,
    epoch: [f64; 3],
    positions: Vec<OnceCell<Option<Apparent>>>,
}
impl Cache {
    fn new(catalog: Rc<Catalog>, epoch: [f64; 3]) -> Self {
        let positions = (0..catalog.len()).map(|_| OnceCell::new()).collect();
        Self {
            catalog,
            epoch,
            positions,
        }
    }
    fn position(&self, row: usize, sky: &SkyData, observer: Observer) -> Option<Apparent> {
        *self.positions[row].get_or_init(|| {
            let Position::Solar(p, _) = self.catalog.locate(row, sky, self.epoch[0]).ok()? else {
                return None;
            };
            observer.observe(p).ok()
        })
    }
}
#[derive(Default)]
pub(super) struct Layer {
    selected: Option<(Kind, usize)>,
    catalogues: [Option<Cache>; 2],
}
impl Layer {
    pub fn focus(&mut self, kind: Kind, active: Option<usize>) {
        self.selected = active.map(|row| (kind, row));
    }
}
pub(crate) struct Canvas<'a> {
    pub chart: &'a Chart<'a>,
    pub painter: &'a Painter,
    pub pointer: Option<Pos2>,
    pub nearest: f32,
    pub opaque: &'a dyn Fn(Pos2) -> bool,
    pub picking: Picking,
}
fn pick(
    candidate: Pos2,
    click: Option<Pos2>,
    nearest: f32,
    opaque: &dyn Fn(Pos2) -> bool,
) -> Option<f32> {
    let distance = candidate.distance_sq(click?);
    (distance <= nearest && !opaque(candidate)).then_some(distance)
}
impl Browser {
    pub(crate) fn paint_horizon(
        &mut self,
        sky: &SkyData,
        epoch: [f64; 3],
        canvas: Canvas<'_>,
    ) -> Option<[f64; 2]> {
        let [jd, lat, lon] = epoch;
        let Ok(observer) = Observer::from_ephemeris(&sky.ephemeris, jd, lat, lon) else {
            return None;
        };
        let mut nearest = canvas.nearest.min(64.);
        let mut picked = None;
        let click = canvas.pointer.filter(|p| !(canvas.opaque)(*p));
        for (slot, kind) in [Kind::Small, Kind::Spacecraft].into_iter().enumerate() {
            if !self.display.group_visible(kind) {
                continue;
            }
            let Some(catalog) = self.scene_catalog(kind, sky, canvas.painter.ctx()) else {
                continue;
            };
            let cached = &mut self.horizon.catalogues[slot];
            if cached
                .as_ref()
                .is_none_or(|c| c.epoch != epoch || !Rc::ptr_eq(&c.catalog, &catalog))
            {
                *cached = Some(Cache::new(catalog.clone(), epoch));
            }
            let cache = cached.as_ref().unwrap();
            for row in 0..catalog.len() {
                let Some(options) = self.display.for_row(&catalog, row) else {
                    continue;
                };
                if !options.points && (kind == Kind::Small || !options.labels) {
                    continue;
                }
                if options.selected_only
                    && !catalog
                        .identity(row)
                        .is_some_and(|(src, i)| self.selections.contains(kind, src, i))
                {
                    continue;
                }
                let Some(observed) = cache.position(row, sky, observer) else {
                    continue;
                };
                let p = canvas.chart.point(observed.horizontal_deg);
                if !canvas.chart.rect.contains(p) {
                    continue;
                }
                let [r, g, b] = if kind == Kind::Small {
                    [255, 149, 0]
                } else {
                    [52, 199, 89]
                };
                let alpha = if observed.horizontal_deg[1] < 0. {
                    102
                } else {
                    255
                };
                let color = Color32::from_rgba_unmultiplied(r, g, b, alpha);
                if options.points {
                    canvas.painter.circle_filled(p, 3., color);
                    if let Some(d) = pick(p, click, nearest, canvas.opaque) {
                        nearest = d;
                        picked = Some((kind, row, catalog.clone(), observed.horizontal_deg));
                    }
                }
                if options.labels {
                    let mut title = catalog.title(row);
                    if kind == Kind::Spacecraft
                        && catalog.craft_path(row).is_some_and(|samples| {
                            samples.first().is_some_and(|s| jd < s.time)
                                || samples.last().is_some_and(|s| jd > s.time)
                        })
                    {
                        title.push_str("（期間外・端点固定）")
                    }
                    canvas.painter.text(
                        p + egui::vec2(7., -12.),
                        Align2::LEFT_TOP,
                        title,
                        FontId::proportional(11.),
                        color,
                    );
                }
                if self.horizon.selected == Some((kind, row)) {
                    canvas
                        .painter
                        .circle_stroke(p, 7., Stroke::new(1.25_f32, Color32::GOLD));
                }
            }
        }
        if canvas.pointer.is_some() {
            self.horizon.selected = picked.as_ref().map(|(k, i, _, _)| (*k, *i));
            if let Some((kind, row, catalog, horizontal)) = picked {
                if canvas.picking == Picking::Click || self.kind != kind || self.active != Some(row)
                {
                    self.select_scene(kind, row, catalog, canvas.picking == Picking::Click);
                }
                return Some(horizontal);
            }
        }
        None
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn considers_source_center_coverage_and_closest_hit_for_all_types() {
        let click = Some(egui::pos2(3., 3.));
        let hidden = |p: Pos2| p.x > 4.;
        assert!(pick(egui::pos2(5., 3.), click, 64., &hidden).is_none());
        assert_eq!(pick(egui::pos2(1., 4.), click, 64., &hidden), Some(5.));
        assert!(pick(egui::pos2(1., 4.), click, 4., &hidden).is_none());
        assert!(pick(egui::pos2(3., 3.), None, 64., &hidden).is_none());
    }

    #[test]
    #[ignore = "requires original Plus 1.5.4 data"]
    fn actual_first_asteroid_and_two_pth_probes_match_native() {
        let root = std::env::var_os("MITAKA_DATA_ROOT").unwrap();
        let data = SkyData::open(std::path::Path::new(&root)).unwrap();
        let epoch = [2451545., 35.68, 139.76];
        let site = Observer::from_ephemeris(&data.ephemeris, epoch[0], epoch[1], epoch[2]).unwrap();
        for (kind, row, reference) in [
            (
                Kind::Small,
                0,
                [
                    188.77516488134174,
                    9.109146754690254,
                    2.2572405211736215,
                    57.73185260047969,
                    -24.050522056346324,
                ],
            ),
            (
                Kind::Spacecraft,
                0,
                [
                    252.90850995161463,
                    -22.523622023930663,
                    1.7970113304746373,
                    316.5876227188703,
                    -72.8294482157273,
                ],
            ),
            (
                Kind::Spacecraft,
                13,
                [
                    292.54536799613635,
                    -46.65438368400499,
                    60.67522572326132,
                    237.5055248567338,
                    -49.90058296709256,
                ],
            ),
        ] {
            let c = Rc::new(Catalog::load(kind, &data.root).unwrap());
            let cache = Cache::new(c, epoch);
            let p = cache.position(row, &data, site).unwrap();
            let [ra, dec] = p.equatorial_deg;
            let [az, alt] = p.horizontal_deg;
            for (actual, expected) in [ra, dec, p.distance_au, az, alt].into_iter().zip(reference) {
                assert!(
                    (actual - expected).abs() < 3e-10,
                    "{kind:?} row={row} {actual} vs {expected}"
                );
            }
        }
    }

    #[test]
    #[ignore = "requires original Plus 1.5.4 data"]
    fn shared_catalog_and_picking_follow_visible_pth_and_panorama_coverage() {
        use crate::projection::{Camera, View};
        use mitaka_core::astronomy::ObservationFrame;
        let root = std::env::var_os("MITAKA_DATA_ROOT").unwrap();
        let sky = SkyData::open(std::path::Path::new(&root)).unwrap();
        let epoch = [2451545., 35.68, 139.76];
        let frame = ObservationFrame::new(epoch[1], epoch[2], epoch[0]).unwrap();
        let mut browser = Browser::default();
        browser.display.craft = true;
        let craft = Rc::new(Catalog::load(Kind::Spacecraft, &sky.root).unwrap());
        browser
            .catalogs
            .insert(&sky.root, Kind::Spacecraft, craft.clone());
        let rect = egui::Rect::from_min_size(Pos2::ZERO, egui::vec2(400., 300.));
        let chart = Chart {
            flight: None,
            view: View::Horizon,
            camera: Camera {
                longitude: 316.5876227188703,
                latitude: -72.8294482157273,
                zoom: 16.,
                roll: 33.,
            },
            frame: &frame,
            rect,
        };
        let ctx = egui::Context::default();
        for (points, labels, covered, expected) in [
            (true, true, false, true),
            (true, true, true, false),
            (false, true, false, false),
        ] {
            browser.display.groups[4].points = points;
            browser.display.groups[4].labels = labels;
            let out = ctx.run(Default::default(), |ctx| {
                let painter = ctx.layer_painter(egui::LayerId::background());
                let selected = browser.paint_horizon(
                    &sky,
                    epoch,
                    Canvas {
                        chart: &chart,
                        painter: &painter,
                        pointer: Some(rect.center()),
                        nearest: 64.,
                        opaque: &|_| covered,
                        picking: Picking::Click,
                    },
                );
                assert_eq!(selected.is_some(), expected);
            });
            assert!(!out.shapes.is_empty());
            if expected {
                assert_eq!(browser.kind, Kind::Spacecraft);
                assert!(browser.active.is_some());
                assert!(Rc::ptr_eq(&craft, browser.data.as_ref().unwrap()));
            }
        }
    }
}
