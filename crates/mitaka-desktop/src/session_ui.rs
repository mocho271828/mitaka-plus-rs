use crate::{
    projection::{Camera, View},
    session::Session,
    App,
};
use serde_json::{json, Value};
use std::{io, path::Path};

impl App {
    pub fn session_dialog(&mut self, save: bool) {
        if self.data.is_none() {
            self.error = Some("先に天文データを開いてください".into());
            return;
        }
        let picker = rfd::FileDialog::new().add_filter("表示状態", &["json"]);
        let path = if save {
            picker.set_file_name("mitaka.json").save_file()
        } else {
            picker.pick_file()
        };
        let Some(path) = path else { return };
        let result = if save {
            self.save_session(&path)
        } else {
            self.restore_session(&path)
        };
        if let Err(e) = result {
            self.error = Some(e.to_string());
        }
    }

    pub(super) fn remembered_session(&mut self, save: bool) -> io::Result<()> {
        if self.data.is_none() {
            return Err(io::Error::other("先に天文データを開いてください"));
        }
        let root = dirs::config_dir()
            .ok_or_else(|| io::Error::other("アプリ設定の保存先を特定できません"))?
            .join("mitaka-plus-rs");
        let path = root.join("remembered-v9.json");
        if save {
            std::fs::create_dir_all(&root)?;
            self.save_session(&path)
        } else if !path.try_exists()? {
            Err(io::Error::other("記憶した表示状態はありません"))
        } else {
            self.restore_session(&path)
        }
    }
    pub(crate) fn save_session(&self, path: &Path) -> io::Result<()> {
        let data = self
            .data
            .as_ref()
            .ok_or_else(|| io::Error::other("先にデータを開いてください"))?;
        let path = crate::capture::output_path(path, &data.root)?;
        let mut state = self.session.clone();
        let v = &mut state.0;
        v["julian_date"] = json!(self.jd);
        v["mode"] = json!(match self.view {
            View::Sky => 0,
            View::Horizon => 2,
            View::Solar | View::Surface => 1,
            View::Galaxy =>
                return Err(io::Error::other("この表示モードの保存には対応していません")),
        });
        v["sky_viewport"] = json!({"center_ra_deg":self.sky.longitude,"center_dec_deg":self.sky.latitude,"zoom":self.sky.zoom});
        v["horizon_viewport"] = json!({"center_azimuth_deg":self.horizon.longitude,"center_altitude_deg":self.horizon.latitude,"zoom":self.horizon.zoom.clamp(1.,180.),"fov_deg":self.horizon_fov(),"roll_deg":self.horizon.roll});
        v["magnitude_limit"] = json!(self.options.magnitude);
        v["observer_latitude_deg"] = json!(self.latitude);
        v["observer_longitude_deg"] = json!(self.longitude);
        self.options.layers.save(v, data);
        v["constellations"]["lines"] = json!(self.options.lines);
        v["constellations"]["labels"] = json!(self.options.labels);
        v["planets"]["orbits"] = json!(self.solar.orbits);
        self.session_camera(v)?;
        v["selected_body"] = json!(if self.view == View::Surface {
            self.surface.as_ref().unwrap().body
        } else {
            self.solar.selected
        });
        self.objects.selections.apply_to(v);
        let layers = &self.solar.layers;
        v["planets"]["visible"] = json!(layers.visible);
        v["planets"]["labels"] = json!(layers.labels);
        v["planets"]["axes"] = json!(layers.axes);
        v["sky_selected_body"] = json!(layers.sky_selected);
        v["horizon_selected_body"] = json!(layers.horizon_selected);
        v["radius_display"]["mode"] = json!(u8::from(!layers.keep_radius));
        for (key, scale) in [
            "space_planet",
            "space_sun",
            "planetarium_planet",
            "planetarium_sun_moon",
        ]
        .into_iter()
        .zip(layers.scales)
        {
            v["radius_display"][key] = json!(scale);
        }
        v["terrain_amplification"] = json!(self.terrain_amplification.stage());
        let surface = &self.solar.surfaces;
        v["earth_layers"] = json!({"clouds":surface.clouds,"city_lights":surface.lights});
        v["eclipse"] = json!({"shadows":surface.eclipse.shadows, "shadow_borders":surface.eclipse.borders,"shadow_umbra":surface.eclipse.umbra,"shadow_penumbra":surface.eclipse.penumbra});
        v["atmosphere"] = json!({"mode":surface.air as u8,"intensity":surface.intensity});
        let p = &self.playback;
        v["plus_time"] = json!({"speed":p.plus_speed,"step":p.plus_step,"value_days":p.plus_value});
        v["simulation_rate"] = json!(p.rate);
        v["show_time_info"] = json!(p.show);
        v["time_settings"] =
            json!({"step":p.gui_step,"custom_seconds":p.seconds,"utc_offset_minutes":p.offset});
        state.write(&path)
    }
    pub(crate) fn restore_session(&mut self, path: &Path) -> io::Result<()> {
        let state = Session::read(path)?;
        let v = &state.0;
        let data = self
            .data
            .as_ref()
            .ok_or_else(|| io::Error::other("先にデータを開いてください"))?;
        let num = |v: &Value, key: &str| -> f64 {
            v[key].as_f64().expect("session validated this number")
        };
        let jd = num(v, "julian_date");
        data.validate_date(jd)?;
        let restored_site = crate::camera_control::session::saved_site(v)?;
        let selections = crate::objects::selections::Selections::load(&data.root, v)?;
        let camera = &v["solar_camera"];
        let mut target: [f64; 3] = std::array::from_fn(|i| camera["target"][i].as_f64().unwrap());
        if let Some(p) = camera.get("precise_target") {
            target = std::array::from_fn(|i| {
                p["anchor_pc"][i].as_f64().unwrap().mul_add(
                    648000. / std::f64::consts::PI,
                    p["offset_au"][i].as_f64().unwrap(),
                )
            });
        }
        let layers = crate::layers::LayerOptions::restore(v, data)?;
        // 検証と出典照合が全て成功してから、補間を停止して一括適用する。
        self.objects = Default::default();
        self.objects.selections = selections;
        self.scripts.cancel();
        self.options.layers = layers;
        self.terrain_amplification = mitaka_core::terrain::Amplification::from_stage(
            v["terrain_amplification"]
                .as_u64()
                .ok_or_else(|| io::Error::other("地形倍率段階が不正です"))?,
        )?;
        self.solar.stop();
        self.set_date(jd);
        self.camera_extras_after_restore(camera, restored_site);
        self.solar.camera.target = target;
        self.solar.camera.distance = num(camera, "distance_au");
        self.solar.camera.yaw = num(camera, "yaw_deg");
        self.solar.camera.pitch = num(camera, "pitch_deg");
        self.solar.camera.roll = num(camera, "roll_deg");
        self.solar.orbits = v["planets"]["orbits"].as_bool().unwrap();
        self.solar.selected = num(v, "selected_body") as usize;
        self.sky = Camera {
            longitude: num(&v["sky_viewport"], "center_ra_deg"),
            latitude: num(&v["sky_viewport"], "center_dec_deg"),
            zoom: num(&v["sky_viewport"], "zoom"),
            roll: 0.,
        };
        self.horizon = Camera {
            longitude: num(&v["horizon_viewport"], "center_azimuth_deg"),
            latitude: num(&v["horizon_viewport"], "center_altitude_deg"),
            zoom: 1.,
            roll: num(&v["horizon_viewport"], "roll_deg"),
        };
        self.set_horizon_fov(num(&v["horizon_viewport"], "fov_deg"));
        self.options.lines = v["constellations"]["lines"].as_bool().unwrap();
        self.options.labels = v["constellations"]["labels"].as_bool().unwrap();
        self.options.magnitude = num(v, "magnitude_limit") as f32;
        self.latitude = num(v, "observer_latitude_deg");
        self.longitude = num(v, "observer_longitude_deg");
        self.view = match num(v, "mode") as i32 {
            0 => View::Sky,
            1 => View::Solar,
            _ => View::Horizon,
        };
        self.selected = None;
        self.surface = None;
        self.galaxies = None;
        let layers = &mut self.solar.layers;
        layers.visible = v["planets"]["visible"].as_bool().unwrap();
        layers.labels = v["planets"]["labels"].as_bool().unwrap();
        layers.axes = v["planets"]["axes"].as_bool().unwrap();
        layers.sky_selected = v["sky_selected_body"].as_f64().map(|i| i as usize);
        layers.horizon_selected = v["horizon_selected_body"].as_f64().map(|i| i as usize);
        layers.keep_radius = num(&v["radius_display"], "mode") == 0.;
        layers.scales = [
            "space_planet",
            "space_sun",
            "planetarium_planet",
            "planetarium_sun_moon",
        ]
        .map(|k| num(&v["radius_display"], k) as f32);
        let surface = &mut self.solar.surfaces;
        surface.clouds = v["earth_layers"]["clouds"].as_bool().unwrap();
        surface.lights = v["earth_layers"]["city_lights"].as_bool().unwrap();
        surface.eclipse = mitaka_core::eclipse::display::Options {
            shadows: v["eclipse"]["shadows"].as_bool().unwrap(),
            borders: v["eclipse"]["shadow_borders"].as_bool().unwrap(),
            umbra: v["eclipse"]["shadow_umbra"].as_bool().unwrap(),
            penumbra: v["eclipse"]["shadow_penumbra"].as_bool().unwrap(),
        };
        surface.air =
            crate::solar::sphere::Air::from_index(num(&v["atmosphere"], "mode") as u64).unwrap();
        surface.intensity = num(&v["atmosphere"], "intensity");
        self.playback = Default::default();
        let p = &mut self.playback;
        p.plus_step = num(&v["plus_time"], "step") as usize;
        p.plus_value = num(&v["plus_time"], "value_days");
        p.rate = num(v, "simulation_rate");
        p.show = v["show_time_info"].as_bool().unwrap();
        p.gui_step = num(&v["time_settings"], "step") as usize;
        p.seconds = num(&v["time_settings"], "custom_seconds") as u32;
        p.offset = num(&v["time_settings"], "utc_offset_minutes") as i32;
        self.session = state;
        Ok(())
    }
}
