use super::{registry::Kind, Media};
use eframe::egui;
use serde_json::{json, Value};
use std::io;
impl Media {
    pub(crate) fn script(&mut self, v: &Value, ctx: &egui::Context) -> Result<Value, String> {
        let key = v["key"].as_str().ok_or("invalid media property")?;
        let op = v["command"].as_str().ok_or("invalid media command")?;
        if v["family"] == "GUI" {
            if key != "soundControler" {
                return Err("unknown media control".into());
            }
            match op {
                "get" => {}
                "set" => self.sound = v["value"].as_bool().ok_or("boolean required")?,
                "call" => self.sound = !self.sound,
                _ => return Err("invalid GUI operation".into()),
            }
            return Ok(json!({"ok":true,"value":self.sound}));
        }
        let kind = match v["family"].as_str() {
            Some("Audio") => Kind::Audio,
            Some("Movies") => Kind::Movie,
            Some("Images") => Kind::Image,
            _ => return Err("unknown media family".into()),
        };
        let slot = Self::slot(kind).map_err(|e| e.to_string())?;
        let ids: Vec<_> = self.registrations.indices(kind).collect();
        let idx = self.selected[slot]
            .filter(|i| ids.contains(i))
            .or_else(|| ids.first().copied());
        let visible = self.active_kind() == Some(kind);
        let run = (|| -> io::Result<Value> {
            let bad = |s: &str| io::Error::other(s);
            let selected = || idx.ok_or_else(|| bad("この種類の登録がありません"));
            let position = || {
                selected().and_then(|i| {
                    ids.iter()
                        .position(|n| *n == i)
                        .ok_or_else(|| bad("登録番号が無効です"))
                })
            };
            let show = match (op, key) {
                ("set", "visible") => Some(
                    v["value"]
                        .as_bool()
                        .ok_or_else(|| bad("真偽値が必要です"))?,
                ),
                ("call", "toggle") => Some(!visible),
                _ => None,
            };
            if let Some(show) = show {
                if show && !visible {
                    self.activate(selected()?, ctx)?;
                } else if !show && visible {
                    self.close();
                }
                return Ok(Value::Null);
            }
            match (op, key) {
                ("get", "visible") => Ok(json!(visible)),
                ("get", "index") => Ok(json!(position()?)),
                ("get", "name") => Ok(json!(&self.registrations.entries[selected()?].name)),
                ("set", "index" | "name") | ("call", "first" | "last" | "prev" | "next") => {
                    if ids.is_empty() {
                        return Err(bad("登録がありません"));
                    }
                    let item = match key {
                        "index" => {
                            let n = v["value"]
                                .as_u64()
                                .and_then(|n| usize::try_from(n).ok())
                                .ok_or_else(|| bad("0始まりの登録番号が必要です"))?;
                            *ids.get(n).ok_or_else(|| bad("登録番号の範囲外です"))?
                        }
                        "name" => {
                            let name =
                                v["value"].as_str().ok_or_else(|| bad("登録名が必要です"))?;
                            let mut rows = ids
                                .iter()
                                .copied()
                                .filter(|&i| self.registrations.entries[i].name == name);
                            let i = rows.next().ok_or_else(|| bad("登録名が見つかりません"))?;
                            if rows.next().is_some() {
                                return Err(bad("登録名が重複しています。番号を指定してください"));
                            }
                            i
                        }
                        "first" => ids[0],
                        "last" => *ids.last().unwrap(),
                        "prev" => ids[position()?.saturating_sub(1)],
                        _ => ids[(position()? + 1).min(ids.len() - 1)],
                    };
                    if visible && idx != Some(item) {
                        self.activate(item, ctx)?;
                    }
                    self.selected[slot] = Some(item);
                    Ok(Value::Null)
                }
                ("get", "playing") if kind != Kind::Image && !visible => Ok(json!(false)),
                _ if kind != Kind::Image && visible => {
                    let w = self
                        .player
                        .as_ref()
                        .ok_or_else(|| bad("プレーヤーがありません"))?;
                    let state = w.request(json!({"op":"status"}))?;
                    match (op, key) {
                        ("get", "playing" | "frame" | "frameLength") => state
                            .get(key)
                            .filter(|v| !v.is_null())
                            .cloned()
                            .ok_or_else(|| bad("この媒体のフレーム単位は未確定です")),
                        ("call", "play" | "pause") => {
                            w.request(json!({"op":key})).map(|_| Value::Null)
                        }
                        ("call", "togglePlaying") => w
                            .request(json!({"op":if state["playing"]==true{"pause"}else{"play"}}))
                            .map(|_| Value::Null),
                        ("set", "playing") => {
                            let p = v["value"]
                                .as_bool()
                                .ok_or_else(|| bad("真偽値が必要です"))?;
                            w.request(json!({"op":if p{"play"}else{"pause"}}))
                                .map(|_| Value::Null)
                        }
                        ("set", "frame") => w
                            .request(json!({"op":"frame","frame":v["value"]}))
                            .map(|_| Value::Null),
                        _ => Err(bad("未対応または読取専用のプロパティです")),
                    }
                }
                _ => Err(bad("選択済み媒体のプレビューを先に開いてください")),
            }
        })()
        .map_err(|e| e.to_string())?;
        Ok(json!({"ok":true,"value":run}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media_preview::registry::Registry;
    #[test]
    fn script_cannot_open_an_unselected_registration() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path();
        let folder = root.join("media");
        let data = root.join("data");
        std::fs::create_dir(&folder).unwrap();
        std::fs::create_dir(&data).unwrap();
        std::fs::write(
            folder.join("images.dat"),
            "a.mvp a.mvp I One first\na.mvp a.mvp I Two second\n",
        )
        .unwrap();
        std::fs::write(folder.join("a.mvp"), "a.png\n").unwrap();
        std::fs::write(root.join("a.png"), [0]).unwrap();
        let mut media = Media {
            registrations: Registry::load(&data),
            ..Default::default()
        };
        let ctx = egui::Context::default();
        let call = |m: &mut Media, cmd: &str, key: &str, value: Value| {
            m.script(
                &json!({"family":"Images","command":cmd,"key":key,"value":value}),
                &ctx,
            )
        };
        assert_eq!(
            call(&mut media, "get", "index", Value::Null).unwrap()["value"],
            0
        );
        assert!(call(&mut media, "set", "visible", json!(true)).is_err());
        call(&mut media, "call", "last", Value::Null).unwrap();
        assert_eq!(
            call(&mut media, "get", "name", Value::Null).unwrap()["value"],
            "Two"
        );
        assert!(call(&mut media, "set", "name", json!("/tmp/arbitrary.png")).is_err());
        assert!(media.active.is_none());
    }
}
