use super::LayerOptions;
use crate::catalog::SkyData;
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::{json, Value};
use std::{collections::BTreeSet, io};
const MARKS: [&str; 3] = ["None", "Binary", "ExoPlanets"];

macro_rules! flags {
    ($(($field:ident,$kind:literal,$host:literal,$group:literal,$key:literal)),+ $(,)?)=>{
        fn write_flags(&self,session:&mut Value){
            $(session[$group][$key]=json!(self.$field);)+
        }
        fn read_flags(&mut self,session:&Value)->io::Result<()>{
            $(self.$field=session[$group][$key].as_bool().ok_or_else(||io::Error::other(concat!("invalid ",$group,".",$key)))?;)+Ok(())
        }
        pub fn snapshot(&self,mut reply:Value)->Value{
            $(reply[$host]=json!(self.$field);)+
            self.deep_sky.snapshot(&mut reply);
            reply["starBrightness"]=json!(self.brightness);reply["starMarking"]=json!(MARKS[self.marking as usize]);reply["landscape"]=json!(self.landscape);reply["milkyWay"]=json!(self.milky_way.name());reply
        }
        fn flag(&mut self,kind:u64)->Option<&mut bool>{
            match kind{$($kind=>Some(&mut self.$field),)+83=>Some(&mut self.landscape),_=>None}
        }
    }
}
impl LayerOptions {
    flags! {
        (circle_scales,35,"circleScales","grids","circle_scales"),
        (square_scales,36,"squareScales","grids","square_scales"),
        (antialias,37,"antialias","renderer","antialias"),
        (point_smoothing,38,"pointSmooth","renderer","point_smoothing"),
        (line_smoothing,39,"lineSmooth","renderer","line_smoothing"),
        (text_smoothing,40,"textSmooth","renderer","text_smoothing"),
        (boundaries,6,"bounds","constellations","boundaries"),(illustrations,65,"illusts","constellations","illustrations"),
        (proper_names,8,"starLabels","stars","proper_names"),(selected_only,66,"selectedStarLabels","stars","selected_proper_names_only"),
        (stars,23,"starsVisible","stars","visible"),(bayer,24,"bayerLabels","stars","bayer_labels"),
        (flamsteed,25,"flamsteedLabels","stars","flamsteed_labels"),(hip,26,"hipLabels","stars","hip_labels"),
        (equatorial,31,"equatorial","grids","equatorial"),(ecliptic,32,"ecliptic","grids","ecliptic"),
        (galactic,33,"galactic","grids","galactic"),(horizontal,34,"horizontal","grids","horizontal")
    }
    pub fn action(&mut self, kind: u64, msg: &Value) -> Result<bool, String> {
        if let Some(flag) = self.deep_sky.flag(kind) {
            *flag = msg["enabled"].as_bool().ok_or("真偽値が必要です")?;
            return Ok(true);
        }
        if let Some(flag) = self.flag(kind) {
            *flag = msg["enabled"].as_bool().ok_or("真偽値が必要です")?;
            return Ok(true);
        }
        if kind == 21 {
            self.milky_way = super::background::Mode::ALL
                .into_iter()
                .find(|mode| msg["text"].as_str() == Some(mode.name()))
                .ok_or("未知の天の川表示")?;
            return Ok(true);
        }
        if kind == 22 {
            self.marking = MARKS
                .iter()
                .position(|&s| msg["text"].as_str() == Some(s))
                .ok_or("未知の恒星マーカー")? as u8;
            return Ok(true);
        }
        let brightness = match kind {
            60 => msg["number"].as_f64().ok_or("数値が必要です")? as f32,
            61 => self.brightness * 10f32.powf(-0.2),
            62 => self.brightness * 10f32.powf(0.2),
            63 => 1.,
            _ => return Ok(false),
        };
        if !brightness.is_finite() || brightness <= 0. {
            return Err("恒星の明るさ係数は正の有限 float が必要です".into());
        }
        self.brightness = brightness;
        Ok(true)
    }
    pub fn save(&self, v: &mut Value, data: &SkyData) {
        self.write_flags(v);
        v["milky_way_mode"] = json!(self.milky_way as u8);
        self.deep_sky.save(v);
        v["stars"]["brightness"] = json!(self.brightness);
        v["stars"]["marking"] = json!(self.marking);
        v["selected_star_name_hips"] = match &self.selected {
            Some(selection) => json!(selection),
            None => json!(data
                .layers
                .featured
                .iter()
                .copied()
                .collect::<BTreeSet<_>>()),
        };
        v["show_landscape_image"] = json!(self.landscape);
        v["observer_place_identifier_base64"] = json!(STANDARD.encode(&self.observer));
        v["constellations"]["default_visible"] = json!(self.show_all_constellations);
        v["constellations"]["exceptions_base64"] = json!(self
            .exceptions
            .iter()
            .map(|s| STANDARD.encode(s))
            .collect::<BTreeSet<_>>());
    }
    pub fn restore(v: &Value, data: &SkyData) -> io::Result<Self> {
        let mut out = Self::default();
        out.read_flags(v)?;
        let mode = v["milky_way_mode"]
            .as_f64()
            .ok_or_else(|| io::Error::other("天の川モードがありません"))?;
        out.milky_way = super::background::Mode::ALL
            .into_iter()
            .find(|m| f64::from(*m as u8) == mode)
            .ok_or_else(|| io::Error::other("天の川モードが範囲外です"))?;
        out.deep_sky = crate::deep_sky::Display::restore(v)?;
        // 呼出元の Session::read は全フィールドの必須性・型・数値範囲を確認済み。
        out.marking = v["stars"]["marking"].as_f64().unwrap() as u8;
        out.brightness = v["stars"]["brightness"].as_f64().unwrap() as f32;
        out.landscape = v["show_landscape_image"].as_bool().unwrap();
        out.show_all_constellations = v["constellations"]["default_visible"].as_bool().unwrap();
        for id in v["constellations"]["exceptions_base64"].as_array().unwrap() {
            let raw = STANDARD
                .decode(id.as_str().unwrap())
                .map_err(io::Error::other)?;
            if !data.constellations.iter().any(|c| c.abbreviation == raw) {
                return Err(io::Error::other(
                    "保存した星座略号が現在のデータにありません",
                ));
            }
            out.exceptions.insert(raw);
        }
        let mut selected = BTreeSet::new();
        for hip in v["selected_star_name_hips"].as_array().unwrap() {
            let hip = hip.as_f64().unwrap() as i32;
            if !data.layers.names.contains_key(&hip) {
                return Err(io::Error::other(
                    "保存した固有名を現在のカタログで照合できません",
                ));
            }
            selected.insert(hip);
        }
        out.selected = Some(selected);
        out.observer = STANDARD
            .decode(v["observer_place_identifier_base64"].as_str().unwrap())
            .map_err(io::Error::other)?;
        if !out.observer.is_empty() {
            let lat = v["observer_latitude_deg"].as_f64().unwrap();
            let lon = v["observer_longitude_deg"].as_f64().unwrap();
            let matches = data.layers.places.places.iter().any(|p| {
                p.earth()
                    && p.id == out.observer
                    && (p.latitude - lat).abs() <= 1e-9
                    && ((p.longitude_signed() - lon + 180.).rem_euclid(360.) - 180.).abs() <= 1e-9
            });
            if !matches {
                return Err(io::Error::other(
                    "観測地点の識別子と座標が現在のカタログに一致しません",
                ));
            }
        }
        Ok(out)
    }
}
