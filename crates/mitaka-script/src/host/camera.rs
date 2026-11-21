//! Camera methods in ScriptWorker.mm use parsecs at the Lua boundary and AU in IPC.
use super::{HostObject, request};
use crate::bindings::AU_PER_PC;
use mlua::{Error, Function, Lua, MultiValue, Result, Value};
use serde_json::json;

const METHODS: &[&str] = &[
    "look",
    "lookTo",
    "setTargetTo",
    "setZoomTo",
    "setZoomLogTo",
    "setFovTo",
    "setAzimuthTo",
    "setAltitudeTo",
    "setModeTo",
    "toggleMode",
    "toggleModeTo",
    "forceMode",
    "getOrient",
    "getEuler",
    "setOrient",
    "setEuler",
    "addEuler",
    "setOrientTo",
    "setEulerTo",
    "addEulerTo",
];
fn error(s: &str) -> Error {
    Error::runtime(s)
}
fn number(v: &Value) -> Result<f64> {
    let n = match v {
        Value::Number(n) => *n,
        Value::Integer(i) => *i as f64,
        _ => return Err(error("expected a number")),
    };
    n.is_finite()
        .then_some(n)
        .ok_or_else(|| error("expected a finite number"))
}
fn boolean(v: &Value) -> Result<bool> {
    match v {
        Value::Boolean(b) => Ok(*b),
        _ => Err(error("expected a boolean")),
    }
}
fn argc(a: &[Value], lo: usize, hi: usize) -> Result<()> {
    if (lo..=hi).contains(&a.len()) {
        Ok(())
    } else {
        Err(error("invalid Camera method argument count"))
    }
}

pub(super) fn method(lua: &Lua, obj: &HostObject, name: &str) -> Result<Option<Function>> {
    let Some(&name) = METHODS.iter().find(|n| **n == name) else {
        return Ok(None);
    };
    let obj = obj.clone();
    lua.create_function(move |_, args: MultiValue| invoke(&obj, name, args))
        .map(Some)
}

fn invoke(obj: &HostObject, name: &str, mut args: MultiValue) -> Result<MultiValue> {
    if let Some(Value::UserData(receiver)) = args.front() {
        if receiver.borrow::<HostObject>()?.name != "Camera" {
            return Err(error("expected Camera receiver"));
        }
        args.pop_front();
    }
    let a = args.into_vec();
    let snapshot = || request(&obj.connection, json!({"op":"snapshot"}));
    let duration = |i: usize, key: &str| -> Result<f64> {
        let d = if let Some(v) = a.get(i) {
            number(v)?
        } else {
            snapshot()?[key]
                .as_f64()
                .ok_or_else(|| error("default camera duration unavailable"))?
        };
        if d.is_finite() && d >= 0. {
            Ok(d)
        } else {
            Err(error("duration must be finite and nonnegative"))
        }
    };
    let mut text = String::new();
    let mut orient = Vec::new();
    let mut n = 0.;
    let mut t = 0.;
    let mut enabled = false;
    let kind = match name {
        "getOrient" | "getEuler" => {
            argc(&a, 0, 0)?;
            let (field, count) = if name == "getOrient" {
                ("cameraOrientation", 4)
            } else {
                ("cameraEuler", 3)
            };
            let reply = snapshot()?;
            let values = reply[field]
                .as_array()
                .filter(|v| v.len() == count)
                .ok_or_else(|| error("orientation unavailable"))?;
            let out = values
                .iter()
                .map(|v| {
                    v.as_f64()
                        .map(Value::Number)
                        .ok_or_else(|| error("invalid orientation reply"))
                })
                .collect::<Result<Vec<_>>>()?;
            return Ok(MultiValue::from_vec(out));
        }
        "setOrient" | "setEuler" | "addEuler" | "setOrientTo" | "setEulerTo" | "addEulerTo" => {
            let count = if name.contains("Orient") { 4 } else { 3 };
            let anim = name.ends_with("To");
            argc(&a, count, count + if anim { 2 } else { 0 })?;
            orient = a[..count].iter().map(number).collect::<Result<_>>()?;
            if anim {
                t = duration(count, "cameraDuration")?
            }
            enabled = a.get(count + 1).map(boolean).transpose()?.unwrap_or(true);
            match name {
                "setOrient" => 94,
                "setEuler" => 95,
                "addEuler" => 96,
                "setOrientTo" => 97,
                "setEulerTo" => 98,
                _ => 99,
            }
        }
        "look" | "lookTo" | "setTargetTo" => {
            let anim = name != "look";
            argc(&a, 1, if anim { 2 } else { 1 })?;
            let Value::String(s) = &a[0] else {
                return Err(error("expected a camera target string"));
            };
            text = s.to_str()?.to_owned();
            if text.len() > 4095 || text.contains('\0') {
                return Err(error("invalid target string"));
            }
            if anim {
                t = duration(1, "cameraDuration")?
            }
            enabled = anim;
            if name == "setTargetTo" { 3 } else { 90 }
        }
        "setAzimuthTo" | "setAltitudeTo" | "setZoomTo" | "setZoomLogTo" | "setFovTo" => {
            let angle = matches!(name, "setAzimuthTo" | "setAltitudeTo");
            argc(&a, 1, if angle { 3 } else { 2 })?;
            n = number(&a[0])?;
            t = duration(1, "cameraDuration")?;
            match name {
                "setAzimuthTo" | "setAltitudeTo" => {
                    enabled = a.get(2).map(boolean).transpose()?.unwrap_or(true);
                    if name == "setAzimuthTo" { 92 } else { 93 }
                }
                "setFovTo" => {
                    if n <= 0. || n > 90. {
                        return Err(error("FOV must be greater than 0 and at most 90 degrees"));
                    }
                    105
                }
                _ => {
                    n *= AU_PER_PC;
                    if !n.is_finite() || n <= 0. {
                        return Err(error("camera distance must be finite and positive"));
                    }
                    enabled = name == "setZoomLogTo";
                    103
                }
            }
        }
        _ => {
            let toggle = name.starts_with("toggle");
            let min = usize::from(!toggle);
            let anim = name.ends_with("To");
            argc(&a, min, min + usize::from(anim))?;
            n = if toggle {
                1. - snapshot()?["cameraMode"]
                    .as_f64()
                    .ok_or_else(|| error("camera mode unavailable"))?
            } else {
                number(&a[0])?
            };
            if n != 0. && n != 1. {
                return Err(error("invalid camera mode"));
            }
            if anim {
                t = duration(min, "cameraModeDuration")?
            }
            enabled = name == "forceMode";
            100
        }
    };
    request(
        &obj.connection,
        json!({"op":"apply","kind":kind,"number":n,"enabled":enabled,"text":text,"duration":t,"hips":[],"radiusScale":[],"orientation":orient}),
    )?;
    Ok(MultiValue::new())
}
