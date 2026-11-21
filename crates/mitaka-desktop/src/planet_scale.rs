//! Mitaka_plus f9fe624 radius_display.cpp と AppController.mm::applyRadiusDisplayOptions。
use crate::planet_layers::PlanetLayers;
use serde_json::Value;

impl PlanetLayers {
    pub(crate) fn scale_action(&mut self, kind: u64, msg: &Value) -> Result<(), String> {
        if kind == 88 {
            self.keep_radius = match msg["number"].as_f64() {
                Some(0.) => true,
                Some(1.) => false,
                _ => return Err("半径倍率モードは0または1です".into()),
            };
            return Ok(());
        }
        let values = msg["radiusScale"]
            .as_array()
            .filter(|a| a.len() == 4)
            .ok_or("半径倍率は4値です")?;
        let mut scales = [0.; 4];
        for (dst, src) in scales.iter_mut().zip(values) {
            let value = src
                .as_f64()
                .filter(|v| *v <= f64::from(f32::MAX) && *v as f32 > 0.)
                .ok_or("半径倍率は正の有限floatです")?;
            *dst = value as f32;
        }
        if scales[0] / 20. == 0. {
            return Err("月の軌道倍率が小さすぎます".into());
        }
        if self.keep_radius && self.scales[..2] != scales[..2] {
            return Err("半径維持モードでの宇宙倍率変更にはカメラ距離の補正が必要です。軌道維持モードを選んでください".into());
        }
        self.scales = scales;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn celestial_radius_mode_updates_all_four_scales_atomically() {
        let mut layers = PlanetLayers::default();
        let scales = |v: [f64; 4]| json!({"radiusScale":v});
        layers.scale_action(87, &scales([4., 3., 2., 5.])).unwrap();
        layers.scale_action(88, &json!({"number":0})).unwrap();
        assert!(layers.scale_action(87, &scales([3., 3., 1., 1.])).is_err());
        assert_eq!(layers.scales, [4., 3., 2., 5.]);
        layers.scale_action(87, &scales([4., 3., 1., 1.])).unwrap();
        layers.scale_action(88, &json!({"number":1})).unwrap();
        assert!(layers
            .scale_action(87, &scales([f64::from(f32::from_bits(1)), 1., 1., 1.]))
            .is_err());
        assert_eq!(layers.scales, [4., 3., 1., 1.]);
    }
}
