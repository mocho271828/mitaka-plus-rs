//! Wryの固定ページが報告するHTMLMediaElementの状態と、親が発行した型付き操作。
use serde_json::{json, Value};
use std::{collections::VecDeque, sync::Mutex};
#[derive(Default)]
pub struct Control {
    pub frames: Option<crate::frames::Frames>,
    state: Mutex<Inner>,
}
#[derive(Default)]
struct Inner {
    seconds: f64,
    duration: Option<f64>,
    playing: bool,
    ready: u8,
    queue: VecDeque<String>,
    reported: bool,
}
impl Control {
    pub fn new(frames: Option<crate::frames::Frames>) -> Self {
        Self {
            frames,
            ..Default::default()
        }
    }
    pub fn native(&self, v: &Value) -> Result<Value, &'static str> {
        let mut c = self.state.lock().map_err(|_| "control lock")?;
        let op = v["op"].as_str().ok_or("missing operation")?;
        let range = c.duration.and_then(|dur| self.frames?.verified(dur));
        if op != "status" {
            if !c.reported || c.ready == 0 {
                return Err("media is not ready");
            }
            let command = match op {
                "play" | "pause" => op.into(),
                "frame" => {
                    let (rate, total) =
                        range.ok_or("verified frame count unavailable for this media")?;
                    let n = v["frame"]
                        .as_f64()
                        .filter(|n| n.is_finite() && *n >= 0. && *n <= f64::from(total))
                        .ok_or("invalid frame index")?;
                    format!("seek {}", n.floor() / rate)
                }
                "seek" => {
                    let n = v["seconds"]
                        .as_f64()
                        .filter(|n| n.is_finite() && *n >= 0.)
                        .ok_or("invalid seek")?;
                    let total = c.duration.ok_or("media duration unavailable")?;
                    format!("seek {}", n.min(total))
                }
                _ => return Err("unsupported player command"),
            };
            if c.queue.len() >= 16 {
                return Err("media control queue is full");
            }
            c.queue.push_back(command);
        }
        Ok(json!({
            "ok": true,
            "reported": c.reported,
            "seconds": c.seconds,
            "duration": c.duration,
            "playing": c.playing,
            "ready": c.ready,
            "frame": range.map(|(rate, total)| (c.seconds * rate + 1e-7).floor().clamp(0., f64::from(total)) as u32),
            "frameLength": range.map(|(_, len)| len),
        }))
    }
    pub fn sync(&self, text: &str) -> Result<String, &'static str> {
        let mut fields = text.split_ascii_whitespace();
        let num = |f: Option<&str>| {
            f.and_then(|f| f.parse::<f64>().ok())
                .filter(|v| v.is_finite())
        };
        let elapsed = num(fields.next())
            .filter(|v| *v >= 0.)
            .ok_or("invalid media time")?;
        let duration = num(fields.next()).ok_or("invalid duration")?;
        let playing = match fields.next() {
            Some("1") => true,
            Some("0") => false,
            _ => return Err("invalid state"),
        };
        let ready = fields
            .next()
            .and_then(|n| n.parse::<u8>().ok())
            .filter(|v| *v <= 4)
            .ok_or("invalid readiness")?;
        if fields.next().is_some() {
            return Err("unexpected report");
        }
        let mut c = self.state.lock().map_err(|_| "control lock")?;
        c.seconds = elapsed;
        c.duration = (duration >= 0.).then_some(duration);
        c.playing = playing;
        c.ready = ready;
        c.reported = true;
        Ok(c.queue.pop_front().unwrap_or_default())
    }
}
