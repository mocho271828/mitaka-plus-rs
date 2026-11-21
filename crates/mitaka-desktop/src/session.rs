//! JSON v9 の検証。未知のキーや保存ファイル内のパスを実行・展開しない。
//! 移植元は Mitaka_plus f9fe624 の session_state.mm / session_layers_json.mm。
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::Value;
use std::{
    collections::BTreeSet,
    fs::File,
    io::{self, Read, Write},
    path::Path,
};
use tempfile::NamedTempFile;

const LIMIT: usize = 1 << 20;
const DEFAULT: &[u8] = include_bytes!("session-default.json");
type Result<T> = io::Result<T>;

fn err(field: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("無効なセッション項目 {field}"),
    )
}
fn number(v: &Value, name: &str) -> Result<f64> {
    v[name]
        .as_f64()
        .filter(|n| n.is_finite())
        .ok_or_else(|| err(name))
}
fn range(v: &Value, name: &str, min: f64, max: f64) -> Result<f64> {
    let n = number(v, name)?;
    if !(min..=max).contains(&n) {
        return Err(err(name));
    }
    Ok(n)
}
fn integer(v: &Value, name: &str, min: i64, max: i64) -> Result<i64> {
    let n = range(v, name, min as f64, max as f64)?;
    if n != n.trunc() {
        return Err(err(name));
    }
    Ok(n as i64)
}
fn vector(v: &Value) -> Result<[f64; 3]> {
    let a = v
        .as_array()
        .filter(|a| a.len() == 3)
        .ok_or_else(|| err("vector"))?;
    let mut result = [0.; 3];
    for (component, raw) in result.iter_mut().zip(a) {
        *component = raw
            .as_f64()
            .filter(|n| n.is_finite())
            .ok_or_else(|| err("vector"))?;
    }
    Ok(result)
}
fn key(v: &Value, allow_empty: bool) -> Result<Vec<u8>> {
    let raw = STANDARD
        .decode(v.as_str().ok_or_else(|| err("base64"))?)
        .map_err(|_| err("base64"))?;
    if raw.contains(&0) || raw.len() > 65536 || (!allow_empty && raw.is_empty()) {
        return Err(err("identifier"));
    }
    Ok(raw)
}
fn array<'a>(v: &'a Value, k: &str) -> Result<&'a [Value]> {
    v[k].as_array().map(Vec::as_slice).ok_or_else(|| err(k))
}

// 現行エンコーダが出力した初期値から必須プロパティの型を検査する。
// 旧v9が省略できるプロパティは別途正規化する。配列の内容は個別に検証する。
fn check_shape(value: &Value, reference: &Value, field: &str) -> Result<()> {
    let good = match reference {
        Value::Object(fields) => {
            let obj = value.as_object().ok_or_else(|| err(field))?;
            for (k, r) in fields {
                check_shape(obj.get(k).ok_or_else(|| err(k))?, r, k)?;
            }
            true
        }
        Value::Array(items) => {
            let a = value.as_array().ok_or_else(|| err(field))?;
            if let Some(item) = items.first() {
                for v in a {
                    check_shape(v, item, field)?;
                }
            }
            true
        }
        Value::Number(_) => value.as_f64().is_some_and(f64::is_finite),
        Value::String(_) => value.is_string(),
        Value::Bool(_) => value.is_boolean(),
        Value::Null => true,
    };
    if good {
        Ok(())
    } else {
        Err(err(field))
    }
}

#[derive(Clone)]
pub struct Session(pub Value);
impl Default for Session {
    fn default() -> Self {
        Self(serde_json::from_slice(DEFAULT).expect("native default fixture is valid JSON"))
    }
}

impl Session {
    pub fn read(path: &Path) -> Result<Self> {
        let f = File::open(path)?;
        if !f.metadata()?.is_file() {
            return Err(err("file type"));
        }
        let mut bytes = Vec::new();
        f.take((LIMIT + 1) as u64).read_to_end(&mut bytes)?;
        Self::decode(&bytes)
    }
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        if bytes.is_empty() || bytes.len() > LIMIT {
            return Err(err("size"));
        }
        let mut v: Value = serde_json::from_slice(bytes)?;
        let ref_state = Self::default();
        if let Some(cam) = v.get_mut("solar_camera").and_then(Value::as_object_mut) {
            for k in ["fov_deg", "roll_deg", "frame_is_orbit"] {
                cam.entry(k)
                    .or_insert_with(|| ref_state.0["solar_camera"][k].clone());
            }
        }
        if let Some(h) = v.get_mut("horizon_viewport").and_then(Value::as_object_mut) {
            if !h.contains_key("fov_deg") {
                let zoom = h
                    .get("zoom")
                    .and_then(Value::as_f64)
                    .filter(|n| (1.0..=180.0).contains(n))
                    .ok_or_else(|| err("zoom"))?;
                h.insert("fov_deg".into(), Value::from(180. / zoom));
            }
            h.entry("roll_deg").or_insert(Value::from(0));
        }
        check_shape(&v, &ref_state.0, "root")?;
        // 保存対象は表示状態だけ。追加フィールドにHTMLの信頼やLua実行文脈を持ち込まない。
        let root = v.as_object_mut().ok_or_else(|| err("root"))?;
        root.retain(|k, _| ref_state.0.get(k).is_some() || k == "surface_site");
        let result = Self(v);
        result.validate()?;
        Ok(result)
    }
    pub fn write(&self, path: &Path) -> Result<()> {
        let bytes = serde_json::to_vec_pretty(&self.0)?;
        Self::decode(&bytes)?;
        let mut out = NamedTempFile::new_in(path.parent().ok_or_else(|| err("path"))?)?;
        out.write_all(&bytes)?;
        out.as_file().sync_all()?;
        out.persist(path)?;
        Ok(())
    }
    fn validate(&self) -> Result<()> {
        let v = &self.0;
        integer(v, "version", 9, 9)?;
        integer(v, "mode", 0, 2)?;
        integer(v, "selected_body", 0, 10)?;
        if !(1.0..=2592000.0).contains(&number(v, "simulation_rate")?.abs()) {
            return Err(err("simulation_rate"));
        }
        for k in ["sky_selected_body", "horizon_selected_body"] {
            if !v[k].is_null() && integer(v, k, 0, 10)? == 3 {
                return Err(err(k));
            }
        }
        range(v, "observer_latitude_deg", -90., 90.)?;
        range(v, "observer_longitude_deg", -180., 180.)?;
        for (k, horizontal, vertical) in [
            ("sky_viewport", "center_ra_deg", "center_dec_deg"),
            (
                "horizon_viewport",
                "center_azimuth_deg",
                "center_altitude_deg",
            ),
        ] {
            let c = &v[k];
            if !(0.0..360.).contains(&number(c, horizontal)?) {
                return Err(err(horizontal));
            }
            let zoom = range(c, "zoom", 1., 180.)?;
            let limit = if k == "sky_viewport" {
                90. - 90. / zoom
            } else {
                90.
            };
            range(c, vertical, -limit, limit)?;
        }
        if range(&v["horizon_viewport"], "fov_deg", 0., 180.)? == 0. {
            return Err(err("fov_deg"));
        }
        let c = &v["solar_camera"];
        vector(&c["target"])?;
        if let Some(p) = c.get("precise_target") {
            let anchor = vector(&p["anchor_pc"])?;
            let offset = vector(&p["offset_au"])?;
            for i in 0..3 {
                if anchor[i].abs() > 1e40
                    || !anchor[i]
                        .mul_add(648000. / std::f64::consts::PI, offset[i])
                        .is_finite()
                {
                    return Err(err("precise_target"));
                }
            }
        }
        let frame = &c["frame"];
        if !frame.is_null() {
            let [a, b, n] = [
                vector(&frame["right"])?,
                vector(&frame["up"])?,
                vector(&frame["outward"])?,
            ];
            let dot = |x: [f64; 3], y: [f64; 3]| x.iter().zip(y).map(|(l, r)| l * r).sum::<f64>();
            let cross = [
                a[1] * b[2] - a[2] * b[1],
                a[2] * b[0] - a[0] * b[2],
                a[0] * b[1] - a[1] * b[0],
            ];
            if [a, b, n]
                .iter()
                .any(|&axis| (dot(axis, axis) - 1.).abs() > 1e-8)
                || [dot(a, b), dot(a, n), dot(b, n)]
                    .iter()
                    .any(|x| x.abs() > 1e-8)
                || dot(cross, n) < 1. - 1e-8
            {
                return Err(err("frame"));
            }
        }
        if c.get("physical_space").is_some_and(|v| !v.is_boolean()) {
            return Err(err("physical_space"));
        }
        let min = if !frame.is_null() && c["frame_is_orbit"] == false {
            1e-12
        } else {
            0.000005
        };
        range(
            c,
            "distance_au",
            min,
            (648000. / std::f64::consts::PI) * 1e40,
        )?;
        range(c, "pitch_deg", -89., 89.)?;
        if range(c, "fov_deg", 0., 90.)? == 0. {
            return Err(err("fov_deg"));
        }
        if let Some(s) = v.get("surface_site") {
            let body = integer(s, "body_index", 1, 10)?;
            range(s, "latitude_deg", -90., 90.)?;
            range(s, "elevation_deg", -90., 90.)?;
            for k in [
                "longitude_deg",
                "heading_deg",
                "roll_deg",
                "terrain_offset_km",
            ] {
                number(s, k)?;
            }
            let above = range(s, "height_km", 0.002, 1e9)? > 0.002 + 1e-9;
            if above != c["physical_space"].as_bool().unwrap_or(false) || (above && body != 3) {
                return Err(err("surface_site.height_km"));
            }
        }
        integer(v, "terrain_amplification", 1, 5)?;
        integer(v, "milky_way_mode", 0, 3)?;
        integer(&v["radius_display"], "mode", 0, 1)?;
        for (obj, prop) in [
            ("radius_display", "space_planet"),
            ("radius_display", "space_sun"),
            ("radius_display", "planetarium_planet"),
            ("radius_display", "planetarium_sun_moon"),
            ("stars", "brightness"),
        ] {
            let n = number(&v[obj], prop)? as f32;
            if !n.is_finite() || n <= 0. {
                return Err(err(prop));
            }
        }
        if number(&v["radius_display"], "space_planet")? as f32 / 20. == 0. {
            return Err(err("space_planet"));
        }
        integer(&v["stars"], "marking", 0, 2)?;
        integer(&v["atmosphere"], "mode", 0, 2)?;
        range(&v["atmosphere"], "intensity", 0., f32::MAX.into())?;
        let ts = &v["time_settings"];
        integer(ts, "step", 0, 10)?;
        integer(ts, "custom_seconds", 1, 2592000)?;
        integer(ts, "utc_offset_minutes", -840, 840)?;
        integer(&v["plus_time"], "step", 0, 11)?;
        integer(&v["plus_time"], "speed", i32::MIN.into(), i32::MAX.into())?;
        if !v["plus_time_zone"].is_null() {
            integer(
                &v["plus_time_zone"],
                "seconds_west_utc",
                i32::MIN.into(),
                i32::MAX.into(),
            )?;
        }
        key(&v["observer_place_identifier_base64"], true)?;
        let mut selected = BTreeSet::new();
        for hip in array(v, "selected_star_name_hips")? {
            let n = hip
                .as_i64()
                .filter(|h| (1..=i64::from(i32::MAX)).contains(h))
                .ok_or_else(|| err("HIP"))?;
            if !selected.insert(n) {
                return Err(err("duplicate HIP"));
            }
        }
        let mut exceptions = BTreeSet::new();
        for x in array(&v["constellations"], "exceptions_base64")? {
            if !exceptions.insert(key(x, false)?) {
                return Err(err("duplicate constellation"));
            }
        }
        let small = &v["small_bodies"];
        if array(small, "kinds")?.len() != 3 {
            return Err(err("small_bodies.kinds"));
        }
        let sampling = &small["orbit_sampling"];
        let span = number(sampling, "last_jd")? - number(sampling, "first_jd")?;
        if !span.is_finite() || span <= 0. {
            return Err(err("orbit_sampling"));
        }
        integer(sampling, "sample_count", 2, 4096)?;
        for layer in [small, &v["satellites"]] {
            validate_selections(layer)?;
        }
        Ok(())
    }
}

/// 非空の表示集合はロード済みカタログの原行・指紋と呼出側で照合してから適用する。
fn validate_selections(layer: &Value) -> Result<()> {
    let mut ids = BTreeSet::new();
    let mut referenced = BTreeSet::new();
    for id in array(layer, "selected_ids")? {
        let source = key(&id["source_key_base64"], false)?;
        let ordinal = integer(id, "record_ordinal", 0, 9_007_199_254_740_991)?;
        referenced.insert(source.clone());
        if !ids.insert((source, ordinal)) {
            return Err(err("duplicate source ID"));
        }
    }
    let mut sources = BTreeSet::new();
    for s in array(layer, "sources")? {
        let source = key(&s["source_key_base64"], false)?;
        integer(s, "byte_count", 0, 9_007_199_254_740_991)?;
        let sha = s["sha256"].as_str().ok_or_else(|| err("sha256"))?;
        if sha.len() != 64
            || !sha
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            || !sources.insert(source)
        {
            return Err(err("source fingerprint"));
        }
    }
    if sources != referenced {
        return Err(err("source references"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_v9_and_optional_fields() {
        let mut native = Session::decode(DEFAULT).unwrap();
        native.0["horizon_viewport"]
            .as_object_mut()
            .unwrap()
            .remove("fov_deg");
        native.0["solar_camera"]
            .as_object_mut()
            .unwrap()
            .remove("frame_is_orbit");
        let restored = Session::decode(&serde_json::to_vec(&native.0).unwrap()).unwrap();
        assert_eq!(restored.0["horizon_viewport"]["fov_deg"], 180.);
        assert_eq!(restored.0["solar_camera"]["frame_is_orbit"], false);
        native.0["html_trust"] = Value::Bool(true);
        assert!(Session::decode(&serde_json::to_vec(&native.0).unwrap())
            .unwrap()
            .0
            .get("html_trust")
            .is_none());
    }
    #[test]
    fn rejects_invalid_state_before_restoring() {
        for (field, bad) in [
            ("version", Value::from(8)),
            ("observer_latitude_deg", Value::from(190)),
            ("simulation_rate", Value::from(0)),
            ("selected_star_name_hips", serde_json::json!([1, 1])),
        ] {
            let mut s = Session::default();
            s.0[field] = bad;
            assert!(Session::decode(&serde_json::to_vec(&s.0).unwrap()).is_err());
        }
        assert!(Session::decode(&vec![b' '; LIMIT + 1]).is_err());
        let mut s = Session::default();
        s.0["satellites"]["sources"] = serde_json::json!([{"source_key_base64":"YQ==","byte_count":12,"sha256":"0".repeat(64)}]);
        assert!(Session::decode(&serde_json::to_vec(&s.0).unwrap()).is_err());
    }
}

#[cfg(test)]
#[test]
#[ignore = "移植元の比較CLIを環境変数 MITAKA_REFERENCE_SESSION で指定"]
fn encoder_round_trips_through_native_decoder() {
    use std::process::{Command, Stdio};
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("session.json");
    let mut current = Session::default();
    current.0["julian_date"] = Value::from(2454567.125);
    current.write(&path).unwrap();
    let native = Command::new(std::env::var_os("MITAKA_REFERENCE_SESSION").unwrap())
        .arg("decode")
        .stdin(Stdio::from(File::open(path).unwrap()))
        .output()
        .unwrap();
    assert!(native.status.success());
    let roundtrip = Session::decode(&native.stdout).unwrap();
    assert_eq!(roundtrip.0["julian_date"], 2454567.125);
}
