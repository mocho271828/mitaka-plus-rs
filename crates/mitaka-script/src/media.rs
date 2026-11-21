//! Plus 1.3 日本語説明書の Movies / Audio / Images の運用可能なプロパティ。
use crate::host::{SharedHost, request};
use mlua::{Error, Lua, MetaMethod, MultiValue, Result, UserData, UserDataMethods, Value};
use serde_json::{Value as Json, json};
#[derive(Clone)]
struct Media {
    family: &'static str,
    host: SharedHost,
}
impl Media {
    fn send(&self, op: &str, key: &str, value: Json) -> Result<Json> {
        let reply = request(
            &self.host,
            json!({"op":"media","family":self.family,"command":op,"key":key,"value":value}),
        )?;
        Ok(reply["value"].clone())
    }
    fn supports(&self, key: &str) -> bool {
        matches!(
            key,
            "visible" | "name" | "index" | "first" | "last" | "next" | "prev" | "toggle"
        ) || self.family != "Images"
            && matches!(
                key,
                "playing" | "frame" | "frameLength" | "play" | "pause" | "togglePlaying"
            )
    }
}
fn result(lua: &Lua, v: Json) -> Result<Value> {
    match v {
        Json::Null => Ok(Value::Nil),
        Json::Bool(b) => Ok(Value::Boolean(b)),
        Json::String(s) => Ok(Value::String(lua.create_string(s)?)),
        Json::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(Value::Integer(i))
            } else {
                Ok(Value::Number(
                    n.as_f64().ok_or_else(|| Error::runtime("invalid number"))?,
                ))
            }
        }
        _ => Err(Error::runtime("invalid media reply")),
    }
}
impl UserData for Media {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_method(MetaMethod::Index, |lua, obj, key: String| {
            if !obj.supports(&key) {
                return Err(Error::runtime("unsupported media property"));
            }
            if matches!(
                key.as_str(),
                "first" | "last" | "next" | "prev" | "toggle" | "play" | "pause" | "togglePlaying"
            ) {
                let me = obj.clone();
                return Ok(Value::Function(lua.create_function(
                    move |_, params: MultiValue| {
                        if params.len() > 1
                            || params
                                .front()
                                .is_some_and(|v| !matches!(v,Value::UserData(u) if u.is::<Media>()))
                        {
                            return Err(Error::runtime("no media method arguments expected"));
                        }
                        me.send("call", &key, Json::Null)?;
                        Ok(())
                    },
                )?));
            }
            result(lua, obj.send("get", &key, Json::Null)?)
        });
        methods.add_meta_method(MetaMethod::NewIndex, |_, obj, (key, v): (String, Value)| {
            if !obj.supports(&key) {
                return Err(Error::runtime("unsupported media property"));
            }
            let value = match (key.as_str(), v) {
                ("name", Value::String(s)) => json!(s.to_str()?.as_ref()),
                ("index", Value::Integer(i)) if i >= 0 => json!(i),
                ("visible" | "playing", Value::Boolean(b)) => json!(b),
                ("frame", Value::Integer(i)) if i >= 0 => json!(i),
                ("frame", Value::Number(n)) if n.is_finite() && n >= 0. => json!(n),
                _ => return Err(Error::runtime("read-only property or invalid media value")),
            };
            obj.send("set", &key, value)?;
            Ok(())
        });
    }
}
pub fn install(lua: &Lua, host: &SharedHost) -> Result<()> {
    for family in ["Movies", "Audio", "Images"] {
        lua.globals().set(
            family,
            Media {
                family,
                host: host.clone(),
            },
        )?;
    }
    Ok(())
}

pub fn gui_get(lua: &Lua, host: &SharedHost, key: &str) -> Result<Option<Value>> {
    match key {
        "soundControler" => {
            let reply = request(
                host,
                json!({"op":"media","family":"GUI","command":"get","key":key}),
            )?;
            Ok(Some(Value::Boolean(reply["value"].as_bool().ok_or_else(
                || Error::runtime("invalid controller state"),
            )?)))
        }
        "toggleSoundControler" => {
            let host = host.clone();
            Ok(Some(Value::Function(lua.create_function(move|_,_:MultiValue|{
            request(&host,json!({"op":"media","family":"GUI","command":"call","key":"soundControler"}))?;Ok(())
        })?)))
        }
        _ => Ok(None),
    }
}
pub fn gui_set(host: &SharedHost, value: Value) -> Result<()> {
    let Value::Boolean(b) = value else {
        return Err(Error::runtime("soundControler expects a boolean"));
    };
    request(
        host,
        json!({"op":"media","family":"GUI","command":"set","key":"soundControler","value":b}),
    )?;
    Ok(())
}
