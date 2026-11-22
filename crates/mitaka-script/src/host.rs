use interprocess::local_socket::{Stream, prelude::*};
use mlua::{Error, Lua, Value as LuaValue};
use serde_json::{Value, json};
use std::{
    cell::RefCell,
    ffi::OsStr,
    io::{self, Read, Write},
    rc::Rc,
};

const FRAME_LIMIT: usize = 65_536;
const LOG_LIMIT: usize = 8192;
// 上限をまたぐUTF-8文字が元のログで完結しているか、最大3バイト先まで確認する。
const LOG_LOOKAHEAD_LIMIT: usize = LOG_LIMIT + 3;
pub type SharedHost = Rc<RefCell<Stream>>;

pub fn connect(endpoint: &OsStr) -> io::Result<SharedHost> {
    #[cfg(unix)]
    let name = endpoint.to_fs_name::<interprocess::local_socket::GenericFilePath>()?;
    #[cfg(windows)]
    let name = endpoint.to_ns_name::<interprocess::local_socket::GenericNamespaced>()?;
    Ok(Rc::new(RefCell::new(Stream::connect(name)?)))
}

fn write_frame(stream: &mut impl Write, value: &Value) -> io::Result<()> {
    let data = serde_json::to_vec(value)?;
    if data.len() > FRAME_LIMIT {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "host frame exceeds 64 KiB",
        ));
    }
    stream.write_all(&(data.len() as u32).to_be_bytes())?;
    stream.write_all(&data)?;
    stream.flush()
}

fn read_frame(stream: &mut impl Read) -> io::Result<Value> {
    let mut header = [0; 4];
    stream.read_exact(&mut header)?;
    let length = u32::from_be_bytes(header) as usize;
    if !(1..=FRAME_LIMIT).contains(&length) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid host frame length",
        ));
    }
    let mut bytes = vec![0; length];
    stream.read_exact(&mut bytes)?;
    Ok(serde_json::from_slice(&bytes)?)
}

fn request(host: &SharedHost, msg: Value) -> mlua::Result<Value> {
    let mut stream = host.borrow_mut();
    write_frame(&mut *stream, &msg).map_err(Error::external)?;
    let reply = read_frame(&mut *stream).map_err(Error::external)?;
    match reply["ok"].as_bool() {
        Some(true) => Ok(reply),
        Some(false) => Err(Error::runtime(
            reply["error"].as_str().unwrap_or("host rejected request"),
        )),
        None => Err(Error::runtime("invalid host reply")),
    }
}

pub fn complete(host: &SharedHost, ok: bool, message: &str) -> io::Result<()> {
    let end = message
        .char_indices()
        .nth(8000)
        .map_or(message.len(), |(i, _)| i);
    write_frame(
        &mut *host.borrow_mut(),
        &json!({"op":"done", "ok":ok, "message":&message[..end]}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_invalid_or_truncated_frames() {
        for header in [0u32, FRAME_LIMIT as u32 + 1, 10] {
            assert!(read_frame(&mut header.to_be_bytes().as_slice()).is_err());
        }
    }
}

use crate::bindings::{self, AU_PER_PC, FIELDS, Field, METHODS, Scalar};
use mlua::{MetaMethod, MultiValue, UserData, UserDataMethods};

#[derive(Clone)]
struct HostObject {
    name: &'static str,
    connection: SharedHost,
}

impl HostObject {
    fn get_field(&self, lua: &Lua, field: &Field) -> mlua::Result<LuaValue> {
        let response = request(&self.connection, json!({"op":"snapshot"}))?;
        let value = &response[field.key];
        match field.scalar {
            Scalar::Bool => value.as_bool().map(LuaValue::Boolean),
            Scalar::Text => {
                if let Some(text) = value.as_str() {
                    return Ok(LuaValue::String(lua.create_string(text)?));
                }
                None
            }
            scalar => value.as_f64().map(|n| {
                LuaValue::Number(if scalar == Scalar::Distance {
                    n / AU_PER_PC
                } else {
                    n
                })
            }),
        }
        .ok_or_else(|| Error::runtime(format!("{} is unavailable", field.key)))
    }

    fn apply(&self, kind: u32, number: f64, enabled: bool, text: &str) -> mlua::Result<()> {
        if !number.is_finite() || text.len() > 4095 || text.contains('\0') {
            return Err(Error::runtime("invalid host action argument"));
        }
        request(
            &self.connection,
            json!({"op":"apply","kind":kind,"number":number,"enabled":enabled,
            "text":text,"duration":0,"hips":[],"radiusScale":[],"orientation":[]}),
        )?;
        Ok(())
    }

    fn set_field(&self, f: &Field, value: LuaValue) -> mlua::Result<()> {
        if !f.writable {
            return Err(Error::runtime("read-only property"));
        }
        match (f.scalar, value) {
            (Scalar::Bool, LuaValue::Boolean(v)) => self.apply(f.action, 0., v, ""),
            (Scalar::Text, LuaValue::String(v)) => self.apply(f.action, 0., false, &v.to_str()?),
            (scalar, value) => {
                let number = match value {
                    LuaValue::Integer(v) => v as f64,
                    LuaValue::Number(v) => v,
                    _ => return Err(Error::runtime("invalid host property type")),
                };
                let n = match scalar {
                    Scalar::Finite => number,
                    Scalar::NonNegative if number >= 0. => number,
                    Scalar::FieldOfView if number > 0. && number <= 90. => number,
                    Scalar::Distance if number > 0. => number * AU_PER_PC,
                    Scalar::Index(max)
                        if number.fract() == 0. && (0. ..=f64::from(max)).contains(&number) =>
                    {
                        number
                    }
                    Scalar::Speed
                        if (f64::from(i32::MIN)..=f64::from(i32::MAX))
                            .contains(&number.trunc()) =>
                    {
                        number.trunc()
                    }
                    _ => return Err(Error::runtime("invalid host property value")),
                };
                self.apply(f.action, n, false, "")
            }
        }
    }

    fn method(
        &self,
        lua: &Lua,
        kind: u32,
        toggle: Option<&'static Field>,
    ) -> mlua::Result<LuaValue> {
        let obj = self.clone();
        Ok(LuaValue::Function(lua.create_function(
            move |lua, args: MultiValue| {
                match args.as_slices() {
                    ([], []) => {}
                    ([LuaValue::UserData(receiver)], [])
                        if receiver.borrow::<HostObject>()?.name == obj.name => {}
                    _ => return Err(Error::runtime("unexpected host method arguments")),
                }
                let enabled = if let Some(f) = toggle {
                    let LuaValue::Boolean(value) = obj.get_field(lua, f)? else {
                        return Err(Error::runtime("expected a boolean flag"));
                    };
                    !value
                } else {
                    false
                };
                obj.apply(kind, 0., enabled, "")
            },
        )?))
    }
}

impl UserData for HostObject {
    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_meta_method(MetaMethod::Index, |lua, obj, key: String| {
            for f in FIELDS.iter().filter(|f| f.object == obj.name) {
                if f.name == key {
                    return obj.get_field(lua, f);
                }
                if f.toggle == Some(key.as_str()) {
                    return obj.method(lua, f.action, Some(f));
                }
            }
            if let Some((_, _, kind)) = METHODS
                .iter()
                .find(|(object, method, _)| *object == obj.name && *method == key)
            {
                return obj.method(lua, *kind, None);
            }
            Err(Error::runtime(format!(
                "unsupported host property: {}.{key}",
                obj.name
            )))
        });
        m.add_meta_method(
            MetaMethod::NewIndex,
            |_, obj, (key, value): (String, LuaValue)| {
                let f = FIELDS
                    .iter()
                    .find(|f| f.object == obj.name && f.name == key)
                    .ok_or_else(|| {
                        Error::runtime(format!("unsupported host property: {}.{key}", obj.name))
                    })?;
                obj.set_field(f, value)
            },
        );
    }
}

pub fn install(lua: &Lua, host: Option<&SharedHost>) -> mlua::Result<()> {
    bindings::constants(lua)?;
    let Some(host) = host else {
        return Ok(());
    };
    let names: std::collections::BTreeSet<_> = FIELDS.iter().map(|f| f.object).collect();
    for name in names {
        lua.globals().set(
            name,
            HostObject {
                name,
                connection: host.clone(),
            },
        )?;
    }
    let host = host.clone();
    lua.globals().set(
        "print",
        lua.create_function(move |_, values: MultiValue| {
            let mut output = Vec::new();
            for (index, value) in values.into_iter().enumerate() {
                if index != 0 {
                    output.push(b'\t');
                }
                if let LuaValue::String(v) = &value {
                    let bytes = v.as_bytes();
                    let remaining = LOG_LOOKAHEAD_LIMIT - output.len();
                    output.extend_from_slice(&bytes[..bytes.len().min(remaining)]);
                } else {
                    let text = match value {
                        LuaValue::Number(v) => format!("{v:.6}"),
                        LuaValue::Integer(v) => format!("{:.6}", v as f64),
                        LuaValue::Boolean(v) => v.to_string(),
                        other => other.type_name().to_owned(),
                    };
                    output.extend_from_slice(text.as_bytes());
                }
                if output.len() >= LOG_LOOKAHEAD_LIMIT {
                    output.truncate(LOG_LOOKAHEAD_LIMIT);
                    break;
                }
            }
            let end = output.len().min(LOG_LIMIT);
            let valid = output
                .utf8_chunks()
                .next()
                .map_or("", |chunk| chunk.valid());
            let text = if valid.len() >= end {
                &valid[..valid.floor_char_boundary(end)]
            } else {
                "[non-UTF-8 output]"
            };
            request(&host, json!({"op":"log","text":text}))?;
            Ok(())
        })?,
    )
}
