use crate::{
    projection::{Camera, View},
    session::Session,
    App,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
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

    fn save_session(&self, path: &Path) -> io::Result<()> {
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
            View::Solar => 1,
            View::Surface | View::Galaxy =>
                return Err(io::Error::other("この表示モードの保存には対応していません")),
        });
        v["sky_viewport"] = json!({"center_ra_deg":self.sky.longitude,"center_dec_deg":self.sky.latitude,"zoom":self.sky.zoom});
        v["horizon_viewport"] = json!({"center_azimuth_deg":self.horizon.longitude,"center_altitude_deg":self.horizon.latitude,"zoom":self.horizon.zoom,"fov_deg":180./self.horizon.zoom,"roll_deg":0.0});
        v["magnitude_limit"] = json!(self.options.magnitude);
        v["observer_latitude_deg"] = json!(self.latitude);
        v["observer_longitude_deg"] = json!(self.longitude);
        v["constellations"]["lines"] = json!(self.options.lines);
        v["constellations"]["labels"] = json!(self.options.labels);
        v["planets"]["orbits"] = json!(self.solar.orbits);
        v["solar_camera"] = json!({"target":self.solar.camera.target, "distance_au":self.solar.camera.distance, "yaw_deg":self.solar.camera.yaw,"pitch_deg":self.solar.camera.pitch, "roll_deg":self.solar.camera.roll,"fov_deg":45.0,"frame":null,"frame_is_orbit":false});
        v["selected_body"] = json!(self.solar.selected);
        state.write(&path)
    }
    fn restore_session(&mut self, path: &Path) -> io::Result<()> {
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
        if v.get("surface_site").is_some()
            || !v["solar_camera"]["frame"].is_null()
            || v["horizon_viewport"]["roll_deg"].as_f64() != Some(0.0)
        {
            return Err(io::Error::other(
                "地表姿勢・傾きを含むセッションは、この版では復元できません",
            ));
        }
        if v["small_bodies"]["selected_ids"]
            .as_array()
            .is_some_and(|a| !a.is_empty())
        {
            return Err(io::Error::other(
                "小天体の選択集合を照合できないため復元を中止しました",
            ));
        }
        let sat = &v["satellites"];
        if sat["selected_ids"]
            .as_array()
            .is_some_and(|a| !a.is_empty())
        {
            use mitaka_core::satellite_catalog::{Dataset, Limits};
            let catalog = Dataset::load(&data.root, Limits::default());
            let source = |value: &Value| {
                let bytes = STANDARD
                    .decode(value["source_key_base64"].as_str().unwrap_or(""))
                    .map_err(io::Error::other)?;
                catalog
                    .sources
                    .iter()
                    .find(|s| s.key.as_bytes() == bytes && s.read_complete)
                    .ok_or_else(|| io::Error::other("衛星データの出典を確認できません"))
            };
            for item in sat["sources"].as_array().unwrap() {
                let s = source(item)?;
                if item["byte_count"].as_f64() != Some(s.raw.len() as f64)
                    || item["sha256"] != format!("{:x}", Sha256::digest(&s.raw))
                {
                    return Err(io::Error::other("衛星の原本が保存時と異なります"));
                }
            }
            for id in sat["selected_ids"].as_array().unwrap() {
                if !source(id)?
                    .records
                    .iter()
                    .any(|r| Some(r.ordinal as f64) == id["record_ordinal"].as_f64())
                {
                    return Err(io::Error::other("衛星の原ブロックを照合できません"));
                }
            }
        }
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
        if camera["fov_deg"].as_f64() != Some(45.) {
            return Err(io::Error::other(
                "太陽系の画角が45度以外の状態には未対応です",
            ));
        }
        // 検証と出典照合が全て成功してから、補間を停止して一括適用する。
        self.solar.stop();
        self.set_date(jd);
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
        };
        self.horizon = Camera {
            longitude: num(&v["horizon_viewport"], "center_azimuth_deg"),
            latitude: num(&v["horizon_viewport"], "center_altitude_deg"),
            zoom: 180. / num(&v["horizon_viewport"], "fov_deg"),
        };
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
        self.session = state;
        Ok(())
    }
}
