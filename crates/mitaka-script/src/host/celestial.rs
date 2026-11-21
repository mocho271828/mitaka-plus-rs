//! Mitaka_plus f9fe624 ScriptWorker.mm の HIP 選択・明るさ・半径倍率と Plus 1.5.4 の Constels。
use super::{HostObject, request};
use mlua::{Error, Function, Lua, MultiValue, Result, Value};
use serde_json::{Value as Json, json};

pub(super) fn method(lua: &Lua, obj: &HostObject, name: &str) -> Result<Option<Function>> {
    let supported: &[&str] = match obj.name {
        "Stars" => &["select", "deselect", "isSelected", "setBrightnessTo"],
        "Constels" => &["show", "hide", "isVisible"],
        "Planets" => &[
            "toggleAtmosphere",
            "setScale",
            "getScale",
            "setScaleMode",
            "getScaleMode",
        ],
        _ => return Ok(None),
    };
    let Some(&method) = supported.iter().find(|m| **m == name) else {
        return Ok(None);
    };
    let obj = obj.clone();
    lua.create_function(move |_, mut args: MultiValue| {
        if let Some(Value::UserData(receiver)) = args.front() {
            if receiver.borrow::<HostObject>()?.name != obj.name {
                return Err(Error::runtime(format!("expected {} receiver", obj.name)));
            }
            args.pop_front();
        }
        invoke(&obj, method, &args.into_vec())
    })
    .map(Some)
}

fn text(v: &Value) -> Result<String> {
    match v {
        Value::String(s) if s.as_bytes().len() <= 4095 && !s.as_bytes().contains(&0) => {
            Ok(s.to_str()?.to_string())
        }
        _ => Err(Error::runtime("expected a string of at most 4095 bytes")),
    }
}
fn hip(text: &str) -> Result<i32> {
    text.bytes()
        .all(|c| c.is_ascii_digit())
        .then(|| text.parse::<i32>().ok())
        .flatten()
        .filter(|&i| i > 0)
        .ok_or_else(|| Error::runtime("expected positive decimal HIP number"))
}
fn number(value: &Value) -> Result<f64> {
    match value {
        Value::Integer(n) => Ok(*n as f64),
        Value::Number(n) if n.is_finite() => Ok(*n),
        _ => Err(Error::runtime("expected a finite number")),
    }
}
fn positive(value: &Value) -> Result<f32> {
    let n = number(value)?;
    if n <= f32::MAX as f64 && n as f32 > 0. {
        Ok(n as f32)
    } else {
        Err(Error::runtime("expected positive float"))
    }
}
fn invoke(obj: &HostObject, name: &str, args: &[Value]) -> Result<MultiValue> {
    let snapshot = || request(&obj.connection, json!({"op":"snapshot"}));
    let scalar = |v| Ok(MultiValue::from_vec(vec![v]));
    let mut action = json!({"op":"apply","number":0,"duration":0,"enabled":false,"hips":[],"radiusScale":[],"text":"","orientation":[]});
    let kind = match (name, args) {
        ("setBrightnessTo", [target, seconds]) => {
            let seconds = number(seconds)?;
            if seconds < 0. {
                return Err(Error::runtime("expected nonnegative duration"));
            }
            action["number"] = json!(positive(target)?);
            action["duration"] = json!(seconds);
            64
        }
        ("select" | "deselect", [s]) => {
            let list = text(s)?;
            action["enabled"] = json!(name == "select");
            if list == "All" {
                68
            } else {
                let hips: Vec<_> = list
                    .split(' ')
                    .filter(|s| !s.is_empty())
                    .map(hip)
                    .collect::<Result<_>>()?;
                if hips.is_empty() {
                    return Err(Error::runtime("expected at least one HIP number"));
                }
                action["hips"] = json!(hips);
                67
            }
        }
        ("isSelected", [s]) => {
            let id = hip(&text(s)?)?;
            let reply = request(&obj.connection, json!({"op":"snapshot","starHIP":id}))?;
            let hips = reply["selectedStarNames"]
                .as_array()
                .ok_or_else(|| Error::runtime("star-name selection unavailable"))?;
            return scalar(Value::Boolean(
                hips.iter().any(|h| h.as_i64() == Some(i64::from(id))),
            ));
        }
        ("show" | "hide" | "isVisible", [s]) => {
            let reply = request(
                &obj.connection,
                json!({"op":"constellations","method":name,"text":text(s)?}),
            )?;
            return if name == "isVisible" {
                scalar(Value::Boolean(reply["visible"].as_bool().ok_or_else(
                    || Error::runtime("missing constellation visibility"),
                )?))
            } else {
                Ok(MultiValue::new())
            };
        }
        ("toggleAtmosphere", []) => {
            let mode = snapshot()?["atmosphereMode"]
                .as_u64()
                .filter(|mode| *mode <= 2)
                .ok_or_else(|| Error::runtime("invalid atmosphere mode"))?;
            action["number"] = json!((mode + 1) % 3);
            71
        }
        ("setScale", [a, b, c, d]) => {
            action["radiusScale"] = json!([positive(a)?, positive(b)?, positive(c)?, positive(d)?]);
            87
        }
        ("getScale", []) => {
            let reply = snapshot()?;
            let values = reply["radiusScale"]
                .as_array()
                .filter(|v| v.len() == 4)
                .ok_or_else(|| Error::runtime("invalid radius scales"))?;
            let out = values
                .iter()
                .map(|v| {
                    v.as_f64()
                        .map(Value::Number)
                        .ok_or_else(|| Error::runtime("invalid radius scale"))
                })
                .collect::<Result<Vec<_>>>()?;
            return Ok(MultiValue::from_vec(out));
        }
        ("setScaleMode", [value]) => {
            let mode = number(value)?;
            if mode != 0. && mode != 1. {
                return Err(Error::runtime("invalid Planets.scaleMode"));
            }
            action["number"] = json!(mode);
            88
        }
        ("getScaleMode", []) => {
            let mode = snapshot()?["radiusScaleMode"]
                .as_f64()
                .filter(|m| *m == 0. || *m == 1.)
                .ok_or_else(|| Error::runtime("invalid radius scale mode"))?;
            return scalar(Value::Number(mode));
        }
        _ => {
            return Err(Error::runtime(format!(
                "invalid arguments to {}:{name}",
                obj.name
            )));
        }
    };
    action["kind"] = Json::from(kind);
    request(&obj.connection, action)?;
    Ok(MultiValue::new())
}
