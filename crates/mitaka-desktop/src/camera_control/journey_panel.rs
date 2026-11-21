use super::{
    journey::{Journey, DURATION},
    session::Site,
};
use crate::{
    projection::View,
    solar::{NAMES, RADII_KM},
    surface::Surface,
    App,
};
use eframe::egui;
use std::{io, time::Instant};

pub(super) struct Panel {
    pub open: bool,
    body: usize,
    lat: f64,
    lon: f64,
    pub flight: Option<Journey>,
}
impl Default for Panel {
    fn default() -> Self {
        Self {
            open: false,
            body: 4,
            lat: 0.,
            lon: 0.,
            flight: None,
        }
    }
}
impl App {
    pub(crate) fn open_journey(&mut self) {
        self.motion.journey.open = true;
    }
    pub(crate) fn journey_panel(&mut self, ctx: &egui::Context) {
        if !self.motion.journey.open {
            return;
        }
        let mut open = true;
        let form = &mut self.motion.journey;
        let mut start = false;
        egui::Window::new("天体間飛行")
            .open(&mut open)
            .show(ctx, |ui| {
                egui::ComboBox::from_label("到着天体")
                    .selected_text(NAMES[form.body])
                    .show_ui(ui, |ui| {
                        for (i, name) in NAMES.iter().enumerate().skip(1) {
                            ui.selectable_value(&mut form.body, i, *name);
                        }
                    });
                ui.horizontal(|ui| {
                    ui.label("緯度");
                    ui.add(
                        egui::DragValue::new(&mut form.lat)
                            .range(-90. ..=90.)
                            .suffix("°"),
                    );
                    ui.label("東経");
                    ui.add(
                        egui::DragValue::new(&mut form.lon)
                            .range(-180. ..=180.)
                            .suffix("°"),
                    );
                });
                if let Some(f) = &form.flight {
                    let phase = if f.elapsed < 4. {
                        "上昇"
                    } else if f.elapsed < 20. {
                        "巡航"
                    } else {
                        "下降"
                    };
                    ui.add(
                        egui::ProgressBar::new((f.elapsed / DURATION) as f32)
                            .text(format!("{}  {phase}", NAMES[f.destination.body])),
                    );
                    if ui.button("現在位置で中断").clicked() {
                        form.flight = None;
                    }
                } else {
                    start = ui.button("現在位置から飛行").clicked();
                }
            });
        self.motion.journey.open = open;
        if start {
            if let Err(e) = self.begin_journey(Instant::now()) {
                self.error = Some(e.to_string());
            }
        }
    }
    pub(crate) fn interrupt_journey(&mut self) {
        self.motion.journey.flight = None;
    }
    fn begin_journey(&mut self, now: Instant) -> io::Result<()> {
        let form = &self.motion.journey;
        let mut dest = Site::on_sphere(form.body, form.lat, form.lon);
        let data = self
            .data
            .as_ref()
            .ok_or_else(|| io::Error::other("天文データを開いてください"))?;
        let mut terrain = None;
        if matches!(dest.body, 3 | 4) {
            let dir = if dest.body == 3 {
                "earth_topo/earth_topo.dat"
            } else {
                "mars_topo/mars_topo.dat"
            };
            if data.root.join(dir).try_exists()? {
                let mesh = Surface::load_mode(
                    &data.root,
                    dest.body,
                    dest.lat,
                    dest.lon,
                    100.,
                    self.terrain_amplification,
                )?;
                dest.terrain = mesh.landing_height_m(RADII_KM[dest.body]) / 1000.;
                terrain = Some(mesh);
            }
        }
        self.solar.prepare(&data.ephemeris, self.jd)?;
        let source = match self.view {
            View::Surface => self
                .surface
                .as_ref()
                .map(|s| Site::from_surface(s, self.horizon)),
            View::Horizon | View::Sky => Some(Site {
                heading: self.horizon.longitude,
                elevation: self.horizon.latitude,
                roll: self.horizon.roll,
                ..Site::on_sphere(3, self.latitude, self.longitude)
            }),
            _ => self.motion.site,
        };
        let centers = self.solar.body_centers()?;
        let flight = Journey::new(source, self.solar.camera, dest, centers, self.jd, now)?;
        self.solar.stop();
        self.solar.camera = flight.start_camera();
        self.solar.selected = dest.body;
        self.motion.site = None;
        self.motion.physical_flight = None;
        self.motion.place = None;
        self.motion.place_name = None;
        self.motion.target = None;
        self.motion.turn = None;
        self.motion.zoom = None;
        self.motion.fov = None;
        self.motion.az = None;
        self.motion.alt = None;
        self.motion.free_physical = true;
        self.motion.journey.flight = Some(flight);
        self.view = View::Solar;
        self.surface = terrain;
        Ok(())
    }
    pub(super) fn tick_journey(&mut self, now: Instant) -> io::Result<()> {
        let Some(mut flight) = self.motion.journey.flight.take() else {
            return Ok(());
        };
        if self.view != View::Solar || self.solar.selected != flight.destination.body {
            return Ok(());
        }
        let data = self
            .data
            .as_ref()
            .ok_or_else(|| io::Error::other("天文データを開いてください"))?;
        self.solar.prepare(&data.ephemeris, self.jd)?;
        self.solar.camera = flight.advance(now, self.solar.body_centers()?, self.jd)?;
        if flight.elapsed < DURATION {
            self.motion.journey.flight = Some(flight);
        } else {
            self.motion.free_physical = false;
            let site = flight.destination;
            if site.body == 3 {
                self.latitude = site.lat;
                self.longitude = site.lon;
                self.options.layers.observer.clear();
            }
            self.motion.site = Some(site);
        }
        Ok(())
    }
}
