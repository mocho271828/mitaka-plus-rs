//! 移植元は native/gui/src/{playback_clock,plus_time_step,calendar_time}.cpp。
use super::{utc_date, App, UNIX_EPOCH_JD};
use chrono::{Datelike, Months, NaiveDate, Utc};
use eframe::egui;
use std::time::{Duration, Instant};

const RATES: [f64; 6] = [1., 60., 3600., 86400., 864000., 2592000.];
const DAYS: [f64; 12] = [
    1. / 86400.,
    10. / 86400.,
    60. / 86400.,
    600. / 86400.,
    1. / 24.,
    1.,
    7.,
    31.,
    365.,
    3650.,
    36500.,
    0.,
];
const LABELS: [&str; 12] = [
    "1秒", "10秒", "1分", "10分", "1時間", "1日", "1週間", "1か月", "1年", "10年", "100年", "任意",
];

struct Clock {
    jd: f64,
    anchor: Instant,
    last: Instant,
    days_per_second: f64,
}

struct TimeAnimation {
    start: Instant,
    from: f64,
    to: f64,
    seconds: f64,
    smooth: bool,
}
impl TimeAnimation {
    fn sample(&self, now: Instant) -> (f64, bool) {
        let elapsed = now.saturating_duration_since(self.start).as_secs_f64();
        if self.seconds <= 0. || elapsed >= self.seconds {
            return (self.to, true);
        }
        let t = elapsed / self.seconds;
        let weight = if self.smooth {
            (1. - (std::f64::consts::PI * t).cos()) * 0.5
        } else {
            t
        };
        (self.from + weight * (self.to - self.from), false)
    }
}

pub(super) struct Playback {
    animation: Option<TimeAnimation>,
    clock: Option<Clock>,
    pub realtime: bool,
    pub show: bool,
    pub rate: f64,
    pub plus_step: usize,
    pub plus_value: f64,
    pub plus_speed: i32,
    pub gui_step: usize,
    pub seconds: u32,
    pub offset: i32,
    month: Option<NaiveDate>,
}
impl Default for Playback {
    fn default() -> Self {
        Self {
            clock: None,
            animation: None,
            realtime: false,
            show: true,
            rate: 3600.,
            plus_step: 4,
            plus_value: DAYS[4],
            plus_speed: 0,
            gui_step: 3,
            seconds: 3600,
            offset: 0,
            month: None,
        }
    }
}
impl Playback {
    pub(super) fn stop(&mut self) {
        self.clock = None;
        self.animation = None;
        self.realtime = false;
        self.plus_speed = 0;
    }
    fn start(&mut self, jd: f64, rate: f64) {
        self.animation = None;
        self.realtime = false;
        let now = Instant::now();
        self.clock = (rate != 0.).then_some(Clock {
            jd,
            anchor: now,
            last: now,
            days_per_second: rate,
        });
    }
    fn sample(&mut self, now: Instant, bounds: [f64; 2]) -> Option<f64> {
        let c = self.clock.as_mut()?;
        if now
            .checked_duration_since(c.last)
            .is_none_or(|d| d > Duration::from_secs(1))
        {
            self.stop();
            return None;
        }
        c.last = now;
        let jd = now
            .duration_since(c.anchor)
            .as_secs_f64()
            .mul_add(c.days_per_second, c.jd);
        let bounded = jd.clamp(bounds[0], bounds[1]);
        if (c.days_per_second > 0. && jd >= bounds[1])
            || (c.days_per_second < 0. && jd <= bounds[0])
        {
            self.stop()
        }
        Some(bounded)
    }
}

fn stepped(
    jd: f64,
    step: usize,
    custom: f64,
    sign: i32,
    offset: i32,
    rollover: bool,
) -> Result<f64, String> {
    let step_days = *DAYS.get(step).ok_or("日時ステップの範囲外です")?;
    if !(7..=10).contains(&step) {
        return Ok(jd + f64::from(sign) * if step == 11 { custom } else { step_days });
    }
    let offset_days = f64::from(offset) / 1440.;
    let dt = utc_date(jd + offset_days).ok_or("暦の範囲外です")?;
    let months = Months::new([1, 12, 120, 1200][step - 7]);
    let base = if rollover {
        dt.with_day(1).ok_or("月初を求められません")?
    } else {
        dt
    };
    let moved = if sign > 0 {
        base.checked_add_months(months)
    } else {
        base.checked_sub_months(months)
    }
    .ok_or("暦の範囲外です")?;
    Ok(jd + (moved - base).num_days() as f64)
}

impl App {
    pub(super) fn apply_date(&mut self, jd: f64) -> Result<(), String> {
        mitaka_core::astronomy::gregorian_date_time(jd).map_err(|e| e.to_string())?;
        if let Some(data) = &self.data {
            data.validate_date(jd).map_err(|e| e.to_string())?
        }
        let dt = utc_date(jd).ok_or("日時の範囲外です")?;
        self.jd = jd;
        self.utc = dt.format("%Y-%m-%d %H:%M:%S").to_string();
        Ok(())
    }
    pub(super) fn animate_time(&mut self, msg: &serde_json::Value) -> Result<(), String> {
        let target = msg["target"].as_f64().ok_or("無効な日時です")?;
        let seconds = msg["duration"].as_f64().ok_or("補間時間が必要です")?;
        let smooth = msg["smooth"].as_bool().ok_or("補間曲線が必要です")?;
        let numeric = msg["numeric"].as_bool().ok_or("日時の形式が必要です")?;
        mitaka_core::astronomy::gregorian_date_time(target).map_err(|e| e.to_string())?;
        if let Some(data) = &self.data {
            data.validate_date(target).map_err(|e| e.to_string())?;
        }
        if numeric && target == self.jd {
            return Ok(());
        }
        self.playback.stop();
        if seconds <= 0. {
            return self.apply_date(target);
        }
        self.playback.animation = Some(TimeAnimation {
            start: Instant::now(),
            from: self.jd,
            to: target,
            seconds,
            smooth,
        });
        Ok(())
    }
    pub(super) fn clock_frame(&mut self, ctx: &egui::Context) {
        let p = &mut self.playback;
        if !p.realtime && p.clock.is_none() && p.animation.is_none() {
            return;
        }
        let bounds = self
            .data
            .as_ref()
            .map_or([37.5, 5373484.49], |d| d.date_range);
        let jd = if let Some(anim) = &p.animation {
            let (jd, done) = anim.sample(Instant::now());
            if done {
                p.animation = None;
            }
            Some(jd)
        } else if p.realtime {
            Some(UNIX_EPOCH_JD + Utc::now().timestamp_millis() as f64 / 86400000.)
        } else {
            p.sample(Instant::now(), bounds)
        };
        if let Some(jd) = jd {
            if let Err(e) = self.apply_date(jd) {
                self.playback.stop();
                self.error = Some(e);
                return;
            }
        }
        ctx.request_repaint_after(Duration::from_millis(33));
    }

    fn date_step(&mut self, plus: bool, sign: i32) -> Result<(), String> {
        let p = &self.playback;
        let jd = if plus {
            stepped(self.jd, p.plus_step, p.plus_value, sign, 0, true)?
        } else {
            stepped(
                self.jd,
                p.gui_step + 1,
                f64::from(p.seconds) / 86400.,
                sign,
                p.offset,
                false,
            )?
        };
        self.apply_date(jd)?;
        self.playback.stop();
        Ok(())
    }
    pub(super) fn time_action(&mut self, kind: u64, msg: &serde_json::Value) -> Result<(), String> {
        let n = || msg["number"].as_f64().ok_or("数値が必要です");
        let enabled = || msg["enabled"].as_bool().ok_or("真偽値が必要です");
        let p = &mut self.playback;
        match kind {
            0 => {
                let jd = n()?;
                self.apply_date(jd)?;
                self.playback.stop();
                return Ok(());
            }
            2 => {
                p.stop();
                return Ok(());
            }
            19 => {
                p.stop();
                p.realtime = enabled()?;
                return Ok(());
            }
            20 => {
                p.show = enabled()?;
                return Ok(());
            }
            72 => {
                let v = n()?;
                if v.fract() != 0. || !(0. ..12.).contains(&v) {
                    return Err("不正な日時ステップです".into());
                }
                p.plus_step = v as usize;
                p.plus_value = DAYS[p.plus_step];
            }
            73 => {
                let v = n()?;
                if !(v * f64::from(p.plus_speed)).is_finite() {
                    return Err("再生速度の範囲外です".into());
                }
                p.plus_value = v;
                p.plus_step = 11;
            }
            74 => {
                let v = n()?.trunc();
                if !(f64::from(i32::MIN)..=f64::from(i32::MAX)).contains(&v)
                    || !(v * p.plus_value).is_finite()
                {
                    return Err("再生速度の範囲外です".into());
                }
                p.plus_speed = v as i32;
            }
            75 => {
                p.plus_step = p.plus_step.saturating_sub(1);
            }
            76 => {
                p.plus_step = (p.plus_step + 1).min(9);
            }
            77 | 78 => return self.date_step(true, if kind == 77 { 1 } else { -1 }),
            _ => return Err("未対応の時刻操作です".into()),
        }
        p.start(self.jd, p.plus_value * f64::from(p.plus_speed));
        Ok(())
    }
    pub(super) fn clock_controls(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            for (label, sign) in [("逆再生", -1.), ("順再生", 1.)] {
                if ui.button(label).clicked() {
                    self.playback.stop();
                    self.playback
                        .start(self.jd, self.playback.rate.abs() * sign / 86400.);
                }
            }
            if ui.button("停止").clicked() {
                self.playback.stop();
            }
            egui::ComboBox::from_id_salt("rate")
                .selected_text(format!("{}倍", self.playback.rate.abs()))
                .show_ui(ui, |ui| {
                    for rate in RATES {
                        if ui
                            .selectable_value(&mut self.playback.rate, rate, format!("{rate}倍"))
                            .changed()
                        {
                            if let Some(c) = &self.playback.clock {
                                let sign = c.days_per_second.signum();
                                self.playback.start(self.jd, rate * sign / 86400.);
                            }
                        }
                    }
                });
            if ui
                .checkbox(&mut self.playback.realtime, "現在時刻に同期")
                .changed()
            {
                self.playback.animation = None;
                self.playback.clock = None;
                self.playback.plus_speed = 0;
            }
        });
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt("date step")
                .selected_text(LABELS[self.playback.gui_step + 1])
                .show_ui(ui, |ui| {
                    for (i, label) in LABELS[1..].iter().enumerate() {
                        ui.selectable_value(&mut self.playback.gui_step, i, *label);
                    }
                });
            if self.playback.gui_step == 10 {
                ui.add(
                    egui::DragValue::new(&mut self.playback.seconds)
                        .range(1..=2592000)
                        .suffix("秒"),
                );
            }
            for (label, sign) in [("前へ", -1), ("次へ", 1)] {
                if ui.button(label).clicked() {
                    if let Err(e) = self.date_step(false, sign) {
                        self.error = Some(e)
                    }
                }
            }
        });
        ui.collapsing("カレンダー", |ui| {
            ui.horizontal(|ui| {
                ui.label("UTC オフセット");
                ui.add(
                    egui::DragValue::new(&mut self.playback.offset)
                        .range(-840..=840)
                        .suffix("分"),
                );
            });
            let zone = &mut self.session.0["plus_time_zone"];
            let mut explicit = !zone.is_null();
            if ui.checkbox(&mut explicit, "Lua の地方時差を指定").changed() {
                *zone = if explicit {
                    serde_json::json!({"seconds_west_utc": -self.playback.offset*60})
                } else {
                    serde_json::Value::Null
                };
            }
            if let Some(mut west) = zone["seconds_west_utc"].as_i64() {
                if ui
                    .add(
                        egui::DragValue::new(&mut west)
                            .range(i32::MIN as i64..=i32::MAX as i64)
                            .prefix("UTC より西へ ")
                            .suffix(" 秒"),
                    )
                    .changed()
                {
                    zone["seconds_west_utc"] = west.into();
                }
            }
            let offset = f64::from(self.playback.offset) / 1440.;
            let Some(local) = utc_date(self.jd + offset) else {
                return;
            };
            let month = self
                .playback
                .month
                .unwrap_or(local.date_naive())
                .with_day(1)
                .unwrap();
            ui.horizontal(|ui| {
                if ui.small_button("◀").clicked() {
                    self.playback.month = month.checked_sub_months(Months::new(1));
                }
                ui.label(month.format("%Y年%m月").to_string());
                if ui.small_button("▶").clicked() {
                    self.playback.month = month.checked_add_months(Months::new(1));
                }
            });
            egui::Grid::new("month").show(ui, |ui| {
                for name in ["日", "月", "火", "水", "木", "金", "土"] {
                    ui.label(name);
                }
                ui.end_row();
                let start = month.weekday().num_days_from_sunday() as i32;
                for slot in 0..42 {
                    let day = slot - start + 1;
                    if let Some(date) = month.with_day(day as u32) {
                        if ui
                            .selectable_label(date == local.date_naive(), day.to_string())
                            .clicked()
                        {
                            let jd = UNIX_EPOCH_JD
                                + date.and_time(local.time()).and_utc().timestamp() as f64 / 86400.
                                - offset;
                            self.set_date(jd);
                        }
                    } else {
                        ui.label("");
                    }
                    if slot % 7 == 6 {
                        ui.end_row();
                    }
                }
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn animation_preserves_endpoints_and_half_cosine() {
        let start = Instant::now();
        let a = TimeAnimation {
            start,
            from: 100.,
            to: 104.,
            seconds: 2.,
            smooth: true,
        };
        assert_eq!(a.sample(start), (100., false));
        assert!((a.sample(start + Duration::from_millis(500)).0 - 100.5857864376).abs() < 1e-8);
        assert_eq!(a.sample(start + Duration::from_secs(2)), (104., true));
    }
    #[test]
    fn clamps_gui_month_and_rolls_plus_month() {
        let jd = super::super::parse_utc("2024-01-31 12:00:00").unwrap();
        assert_eq!(stepped(jd, 7, 0., 1, 0, false).unwrap(), jd + 29.);
        assert_eq!(stepped(jd, 7, 0., 1, 0, true).unwrap(), jd + 31.);
    }
    #[test]
    fn clock_stops_on_sleep_and_at_range_edges() {
        let mut p = Playback::default();
        p.start(100., -1.);
        let anchor = p.clock.as_ref().unwrap().anchor;
        assert_eq!(
            p.sample(anchor + Duration::from_millis(500), [99.75, 101.]),
            Some(99.75)
        );
        assert!(p.clock.is_none());
        p.start(100., 1.);
        let a = p.clock.as_ref().unwrap().anchor;
        assert!(p.sample(a + Duration::from_secs(2), [0., 200.]).is_none());
        assert!(p.clock.is_none());
    }
}
