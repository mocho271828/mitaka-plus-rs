//! Mitaka_plus の plus_time_step.cpp、plus_time_calendar.cpp、ScriptWorker.mm が定義する Lua 暦。
use super::{HostObject, request};
use mitaka_core::astronomy::{GregorianDateTime, gregorian_date_time, julian_date};
use mlua::{Error, Lua, MultiValue, Result, Value};
use serde_json::json;
use std::time::{SystemTime, UNIX_EPOCH};

const COMPONENTS: [&str; 8] = [
    "year", "month", "day", "hour", "minute", "second", "min", "sec",
];

fn component(key: &str) -> Option<(bool, usize)> {
    for (i, name) in COMPONENTS.iter().enumerate() {
        let index = if i >= 6 { i - 2 } else { i };
        if key == *name {
            return Some((false, index));
        }
        if key.strip_prefix("local").is_some_and(|s| {
            s.as_bytes().first().is_some_and(u8::is_ascii_uppercase) && s.eq_ignore_ascii_case(name)
        }) {
            return Some((true, index));
        }
    }
    None
}
fn finite(v: &Value) -> Result<f64> {
    let n = match v {
        Value::Integer(i) => *i as f64,
        Value::Number(n) => *n,
        _ => return Err(Error::runtime("expected a number")),
    };
    n.is_finite()
        .then_some(n)
        .ok_or_else(|| Error::runtime("expected a finite number"))
}
fn integer(v: &Value) -> Result<i32> {
    let n = finite(v)?;
    if n.trunc() != n || !(i32::MIN as f64..=i32::MAX as f64).contains(&n) {
        return Err(Error::runtime("expected an int32"));
    }
    Ok(n as i32)
}
fn date_to_jd(c: [i32; 6]) -> Result<f64> {
    let [year, month, day, hour, minute, second] = c;
    if !(-4712..=9999).contains(&year) || !c[1..5].iter().all(|v| i8::try_from(*v).is_ok()) {
        return Err(Error::runtime("calendar component outside original range"));
    }
    let m = month - 1;
    let jd = julian_date(GregorianDateTime {
        year: year + m.div_euclid(12),
        month: (m.rem_euclid(12) + 1) as u8,
        day: 1,
        hour: 0,
        minute: 0,
        second: 0.,
    })
    .map_err(Error::external)?
        + f64::from(day - 1)
        + (f64::from(hour) * 3600. + f64::from(minute) * 60. + f64::from(second)) / 86400.;
    gregorian_date_time(jd).map_err(Error::external)?;
    Ok(jd)
}
fn time_parts(jd: f64, utc: f64, round_utc: bool) -> Result<[i32; 6]> {
    let date = gregorian_date_time(jd).map_err(Error::external)?;
    let fields = |d: GregorianDateTime| {
        [
            d.year,
            d.month.into(),
            d.day.into(),
            d.hour.into(),
            d.minute.into(),
            d.second as i32,
        ]
    };
    if round_utc {
        let sec = ((jd + 0.5) * 86400.).round() as i64;
        let mut result = fields(
            gregorian_date_time(sec.div_euclid(86400) as f64 - 0.5).map_err(Error::external)?,
        );
        let offset = sec.rem_euclid(86400) as i32;
        result[3..].copy_from_slice(&[offset / 3600, offset % 3600 / 60, offset % 60]);
        Ok(result)
    } else {
        Ok(fields(
            gregorian_date_time(jd + allowance(jd, utc)).unwrap_or(date),
        ))
    }
}
fn allowance(jd: f64, utc: f64) -> f64 {
    let ulp = |x: f64| x.next_up() - x;
    if jd == utc {
        ulp(jd)
    } else {
        2. * ulp(jd).max(ulp(utc))
    }
}
fn snapshot(obj: &HostObject, local: bool) -> Result<(f64, f64)> {
    let reply = request(&obj.connection, json!({"op":"snapshot"}))?;
    let jd = reply["jd"]
        .as_f64()
        .ok_or_else(|| Error::runtime("missing Julian date"))?;
    let offset = if local {
        reply["plusTimeZoneWestSeconds"]
            .as_i64()
            .and_then(|v| i32::try_from(v).ok())
            .ok_or_else(|| Error::runtime("local Time requires an explicit CRT time zone"))?
            as f64
            / 24.
            / 3600.
    } else {
        0.
    };
    Ok((jd, offset))
}
fn set(obj: &HostObject, jd: f64) -> Result<()> {
    gregorian_date_time(jd).map_err(Error::external)?;
    obj.apply(0, jd, false, "")
}
pub(super) fn set_component(obj: &HostObject, key: &str, value: &Value) -> Result<bool> {
    let Some((local, index)) = component(key) else {
        return Ok(false);
    };
    let number = integer(value)?;
    let (jd, offset) = snapshot(obj, local)?;
    let local_jd = jd - offset;
    let mut parts = time_parts(local_jd, jd, !local)?;
    let mut fraction = 0.;
    if local {
        let d = gregorian_date_time(local_jd).map_err(Error::external)?;
        if (local_jd - date_to_jd(parts)?).abs() > allowance(local_jd, jd) {
            parts = [
                d.year,
                d.month.into(),
                d.day.into(),
                d.hour.into(),
                d.minute.into(),
                d.second as i32,
            ];
            fraction = d.second.fract();
        }
        if parts[index] == number && (index != 5 || fraction == 0.) {
            return Ok(true);
        }
    }
    parts[index] = number;
    set(
        obj,
        date_to_jd(parts)? + (if index == 5 { 0. } else { fraction }) / 86400. + offset,
    )?;
    Ok(true)
}
fn parse_string(mut bytes: &[u8]) -> Result<[i32; 6]> {
    if bytes.contains(&0) {
        return Err(Error::runtime("embedded NUL in calendar"));
    }
    let mut parts = [0; 6];
    for (index, out) in parts.iter_mut().enumerate() {
        bytes = bytes.trim_ascii_start();
        let negative = bytes.first() == Some(&b'-');
        if matches!(bytes.first(), Some(b'+' | b'-')) {
            bytes = &bytes[1..]
        }
        let radix = if bytes.starts_with(b"0x") || bytes.starts_with(b"0X") {
            bytes = &bytes[2..];
            16
        } else if bytes.first() == Some(&b'0') {
            8
        } else {
            10
        };
        let len = bytes
            .iter()
            .take_while(|&&v| char::from(v).is_digit(radix))
            .count();
        let digits = std::str::from_utf8(&bytes[..len]).map_err(Error::external)?;
        let magnitude = i64::from_str_radix(digits, radix).map_err(Error::external)?;
        *out = i32::try_from(if negative { -magnitude } else { magnitude })
            .map_err(Error::external)?;
        bytes = &bytes[len..];
        if index != 5 {
            bytes = bytes
                .strip_prefix(b",")
                .ok_or_else(|| Error::runtime("expected comma"))?;
        }
    }
    Ok(parts)
}
fn local_method(key: &str) -> Option<(bool, String)> {
    let (set, suffix) = if let Some(s) = key.strip_prefix("getLocal") {
        (false, s)
    } else {
        (true, key.strip_prefix("setLocal")?)
    };
    let prop = format!("local{suffix}");
    component(&prop).map(|_| (set, prop))
}
pub(super) fn get(lua: &Lua, obj: &HostObject, key: &str) -> Result<Option<Value>> {
    if let Some((local, index)) = component(key) {
        let (jd, offset) = snapshot(obj, local)?;
        return Ok(Some(Value::Integer(i64::from(
            time_parts(jd - offset, jd, !local)?[index],
        ))));
    }
    if local_method(key).is_none()
        && !matches!(
            key,
            "setTime"
                | "getTime"
                | "setLocalTime"
                | "getLocalTime"
                | "setTimeString"
                | "getTimeString"
                | "setJDTo"
                | "setTimeTo"
                | "setLocalTimeTo"
                | "setCurrent"
        )
    {
        return Ok(None);
    }
    let obj = obj.clone();
    let name = key.to_owned();
    Ok(Some(Value::Function(lua.create_function(
        move |lua, args| invoke(lua, &obj, &name, args),
    )?)))
}
fn invoke(lua: &Lua, obj: &HostObject, name: &str, mut args: MultiValue) -> Result<MultiValue> {
    if let Some(Value::UserData(v)) = args.front() {
        if v.borrow::<HostObject>()?.name != "Time" {
            return Err(Error::runtime("expected Time receiver"));
        }
        args.pop_front();
    }
    let a = args.into_vec();
    let argc = |count: usize| {
        if a.len() == count {
            Ok(())
        } else {
            Err(Error::runtime("invalid Time method argument count"))
        }
    };
    if let Some((set, prop)) = local_method(name) {
        argc(usize::from(set))?;
        if set {
            set_component(obj, &prop, &a[0])?;
        } else {
            return Ok(MultiValue::from_vec(vec![get(lua, obj, &prop)?.unwrap()]));
        }
    } else if name == "setTimeString" {
        argc(1)?;
        let Value::String(s) = &a[0] else {
            return Err(Error::runtime("expected calendar string"));
        };
        set(obj, date_to_jd(parse_string(&s.as_bytes())?)?)?;
        return Ok(MultiValue::from_vec(a));
    } else if name.starts_with("get") {
        argc(0)?;
        let (jd, offset) = snapshot(obj, name == "getLocalTime")?;
        let p = time_parts(jd - offset, jd, name == "getTime")?;
        if name == "getTimeString" {
            let text = p.map(|n| n.to_string()).join(",");
            return Ok(MultiValue::from_vec(vec![Value::String(
                lua.create_string(text)?,
            )]));
        }
        return Ok(MultiValue::from_vec(
            p.into_iter().map(|v| Value::Integer(v.into())).collect(),
        ));
    } else if name == "setCurrent" {
        argc(0)?;
        let t = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(Error::external)?;
        set(obj, 2440587.5 + t.as_secs() as f64 / 86400.)?;
    } else {
        let anim = name.ends_with("To");
        let numeric = name == "setJDTo";
        let size = if numeric { 1 } else { 6 };
        if !anim {
            argc(size)?
        } else if a.len() != size + 1 && a.len() != size + 2 {
            return Err(Error::runtime("invalid time animation arguments"));
        }
        let target = if numeric {
            finite(&a[0])?
        } else {
            let mut p = [0; 6];
            for (out, v) in p.iter_mut().zip(&a) {
                *out = integer(v)?;
            }
            date_to_jd(p)?
                + if name.contains("Local") {
                    snapshot(obj, true)?.1
                } else {
                    0.
                }
        };
        gregorian_date_time(target).map_err(Error::external)?;
        if anim {
            let smooth = match a.get(size + 1) {
                None => true,
                Some(Value::Boolean(b)) => *b,
                _ => return Err(Error::runtime("expected a boolean curve")),
            };
            request(
                &obj.connection,
                json!({"op":"time_animation","target":target,"duration":finite(&a[size])?,"smooth":smooth,"numeric":numeric}),
            )?;
        } else {
            set(obj, target)?;
        }
    }
    Ok(MultiValue::new())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn calendar_matches_rollover_and_original_string_formats() {
        let c = parse_string(b"0x7d0,01,040,03,04,05suffix\xff").unwrap();
        assert_eq!(c, [2000, 1, 32, 3, 4, 5]);
        let jd = date_to_jd(c).unwrap();
        assert_eq!(time_parts(jd, jd, true).unwrap(), [2000, 2, 1, 3, 4, 5]);
        assert!(parse_string(b"2000,1,1,0,0,2147483648").is_err());
        assert!(parse_string(b"2000 ,1,1,0,0,0").is_err());
        assert!(date_to_jd([2000, 128, 1, 0, 0, 0]).is_err());
    }
}
