//! Mitaka_plus f9fe624 の StarBrightnessTransition と AppScripting の HIP フィルター。
use super::{LayerOptions, LayersCatalog};
use mitaka_core::catalog::Constellation;
use serde_json::{json, Value};
use std::{collections::BTreeSet, time::Instant};

pub(super) struct Fade {
    from: f32,
    to: f32,
    duration: f64,
    start: Instant,
}

impl LayerOptions {
    pub(crate) fn animate_brightness(
        &mut self,
        target: f64,
        seconds: f64,
        now: Instant,
    ) -> Result<(), String> {
        if target > f64::from(f32::MAX)
            || !(target as f32).is_finite()
            || target as f32 <= 0.
            || !seconds.is_finite()
            || seconds < 0.
        {
            return Err("恒星の明るさ係数は正の有限float、秒数は0以上の有限値です".into());
        }
        let to = target as f32;
        if self.brightness == to {
            return Ok(());
        }
        if seconds == 0. {
            self.brightness = to;
            self.fade = None;
        } else {
            self.fade = Some(Fade {
                from: self.brightness,
                to,
                duration: seconds,
                start: now,
            });
        }
        Ok(())
    }
    pub(crate) fn stop_brightness(&mut self) {
        self.fade = None;
    }
    pub(crate) fn sample_brightness(&mut self, now: Instant) -> bool {
        let Some(fade) = &self.fade else { return false };
        let elapsed = now.saturating_duration_since(fade.start).as_secs_f64();
        if elapsed >= fade.duration {
            self.brightness = fade.to;
            self.fade = None;
            return false;
        }
        let weight = 0.5 - 0.5 * (std::f64::consts::PI * elapsed / fade.duration).cos();
        self.brightness =
            ((1. - weight) * f64::from(fade.from) + weight * f64::from(fade.to)) as f32;
        true
    }
    pub(crate) fn star_selection(
        &self,
        c: Option<&LayersCatalog>,
        hip: Option<i64>,
    ) -> Option<Vec<i32>> {
        // 一件照会では選択集合の大きさにかかわらず IPC フレームの上限を守る。
        if let Some(hip) = hip {
            let hip = i32::try_from(hip).ok()?;
            let selected = self.selected.as_ref().map_or_else(
                || c.is_some_and(|c| c.featured.contains(&hip)),
                |s| s.contains(&hip),
            );
            return Some(if selected { vec![hip] } else { vec![] });
        }
        let count = self
            .selected
            .as_ref()
            .map_or_else(|| c.map_or(0, |c| c.featured.len()), BTreeSet::len);
        if count > 4096 {
            return None;
        }
        Some(
            self.selected
                .clone()
                .unwrap_or_else(|| {
                    c.into_iter()
                        .flat_map(|c| c.featured.iter().copied())
                        .collect()
                })
                .into_iter()
                .collect(),
        )
    }
    pub(crate) fn select_stars(
        &mut self,
        c: &LayersCatalog,
        hips: Option<&[Value]>,
        enabled: bool,
    ) -> Result<(), String> {
        if let Some(hips) = hips {
            let ids = hips
                .iter()
                .map(|h| {
                    h.as_i64()
                        .and_then(|v| i32::try_from(v).ok())
                        .filter(|i| *i > 0)
                        .ok_or("正のHIP番号が必要です")
                })
                .collect::<Result<Vec<_>, _>>()?;
            let selection = self
                .selected
                .get_or_insert_with(|| c.featured.iter().copied().collect());
            for hip in ids.into_iter().filter(|h| c.known_star_names.contains(h)) {
                if enabled {
                    selection.insert(hip);
                } else {
                    selection.remove(&hip);
                }
            }
        } else {
            self.selected = Some(if enabled {
                c.known_star_names.iter().copied().collect()
            } else {
                BTreeSet::new()
            });
        }
        Ok(())
    }
    pub(crate) fn set_constellation(&mut self, id: &[u8], visible: bool) {
        if visible == self.show_all_constellations {
            self.exceptions.remove(id);
        } else {
            self.exceptions.insert(id.to_vec());
        }
    }
    pub(crate) fn constellation_request(
        &mut self,
        catalog: &[Constellation],
        msg: &Value,
    ) -> Result<Value, String> {
        let method = msg["method"].as_str().ok_or("星座操作がありません")?;
        let list = msg["text"]
            .as_str()
            .filter(|s| s.len() <= 4095)
            .ok_or("星座略号が必要です")?;
        let lookup = |s: &str| {
            catalog
                .iter()
                .find(|c| c.abbreviation.as_slice() == s.as_bytes())
                .map(|c| c.abbreviation.as_slice())
        };
        let visible = match method {
            "show" => true,
            "hide" => false,
            "isVisible" => {
                return Ok(
                    json!({"ok":true,"visible":lookup(list).is_some_and(|id|self.shows(id))}),
                )
            }
            _ => return Err("未知の星座操作".into()),
        };
        if list == "All" {
            self.show_all_constellations = visible;
            self.exceptions.clear();
        } else {
            // 旧版の略号の大文字・小文字をそのまま照合し、変更前にリスト全体を検査する。
            let ids = list
                .split(' ')
                .filter(|s| !s.is_empty())
                .map(|s| lookup(s).ok_or_else(|| format!("不明な星座略号: {s}")))
                .collect::<Result<Vec<_>, _>>()?;
            if ids.is_empty() {
                return Err("少なくとも1つの星座略号が必要です".into());
            }
            for id in ids {
                self.set_constellation(id, visible);
            }
        }
        Ok(json!({"ok":true}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn half_cosine_uses_monotonic_time_and_rejects_invalid_replacement() {
        let mut layer = LayerOptions::default();
        let start = Instant::now();
        layer.animate_brightness(9., 4., start).unwrap();
        assert!(layer.sample_brightness(start + Duration::from_secs(1)));
        let quarter = (1. + 4. * (1. - std::f64::consts::FRAC_PI_4.cos())) as f32;
        assert!((layer.brightness - quarter).abs() < 1e-6);
        assert!(layer.animate_brightness(f64::MAX, 0., start).is_err());
        assert!(layer.sample_brightness(start + Duration::from_secs(2)));
        assert_eq!(layer.brightness, 5.);
        assert!(!layer.sample_brightness(start + Duration::from_secs(4)));
        assert_eq!(layer.brightness, 9.);
        layer.animate_brightness(3., 1., start).unwrap();
        assert!(layer.action(60, &json!({"number":2})).unwrap());
        assert!(!layer.sample_brightness(start + Duration::from_secs(1)));
        assert_eq!(layer.brightness, 2.);
    }

    #[test]
    fn unknown_hips_are_filtered_after_validating_the_whole_list() {
        let c = LayersCatalog {
            known_star_names: [1, 2, 3].into(),
            featured: [1].into(),
            ..Default::default()
        };
        let mut layer = LayerOptions::default();
        layer
            .select_stars(&c, Some(&[json!(2), json!(12345)]), true)
            .unwrap();
        assert_eq!(layer.star_selection(Some(&c), None), Some(vec![1, 2]));
        assert!(layer
            .select_stars(&c, Some(&[json!(1), json!(-1)]), false)
            .is_err());
        assert_eq!(layer.star_selection(Some(&c), Some(1)), Some(vec![1]));
        layer.select_stars(&c, None, false).unwrap();
        assert_eq!(layer.star_selection(Some(&c), None), Some(vec![]));
        layer.select_stars(&c, None, true).unwrap();
        assert_eq!(layer.selected.unwrap(), [1, 2, 3].into());
    }

    #[test]
    #[ignore = "requires the local Plus catalog in MITAKA_DATA_ROOT"]
    fn celestial_selections_match_old_twelve_constellations_and_round_trip() {
        let root = std::env::var_os("MITAKA_DATA_ROOT").unwrap();
        let data = crate::catalog::SkyData::open(std::path::Path::new(&root)).unwrap();
        let mut layer = LayerOptions::default();
        let twelve = "Aqr Ari Cap Cnc Gem Leo Lib Psc Sco Sgr Tau Vir";
        layer
            .constellation_request(&data.constellations, &json!({"method":"hide","text":"All"}))
            .unwrap();
        layer
            .constellation_request(
                &data.constellations,
                &json!({"method":"show","text":twelve}),
            )
            .unwrap();
        assert_eq!(
            data.constellations
                .iter()
                .filter(|c| layer.shows(&c.abbreviation))
                .count(),
            12
        );
        let invalid = layer.constellation_request(
            &data.constellations,
            &json!({"method":"show","text":"Ori UNKNOWN"}),
        );
        assert!(invalid.is_err() && !layer.shows(b"Ori") && layer.shows(b"Aqr"));
        layer.select_stars(&data.layers, None, false).unwrap();
        layer
            .select_stars(&data.layers, Some(&[json!(32349), json!(100000000)]), true)
            .unwrap();
        layer.selected_only = true;
        assert!(layer.includes_name(32349, &data.layers));
        let mut session = crate::session::Session::default();
        layer.save(&mut session.0, &data);
        let restored = LayerOptions::restore(&session.0, &data).unwrap();
        assert_eq!(layer.selected, restored.selected);
        assert_eq!(layer.exceptions, restored.exceptions);
        assert_eq!(
            restored.star_selection(Some(&data.layers), None),
            Some(vec![32349])
        );
        assert!(!restored.shows(b"Ori") && restored.shows(b"Aqr"));
    }
}
