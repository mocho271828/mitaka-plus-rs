//! Mitaka 1.3.1 の星座ID順序と Mitaka_plus f9fe624 star_labels.cpp / star_brightness.cpp。
use super::{LayerOptions, LayersCatalog};
use mitaka_core::catalog::Star;
const ABBR: [&str; 88] = [
    "And", "Mon", "Sgr", "Del", "Ind", "Psc", "Lep", "Boo", "Hya", "Eri", "Tau", "CMa", "Lup",
    "UMa", "Vir", "Ari", "Ori", "Pic", "Cas", "Dor", "Cnc", "Com", "Cha", "Crv", "CrB", "Tuc",
    "Aur", "Cam", "Pav", "Cet", "Cep", "Cen", "Mic", "CMi", "Equ", "Vul", "UMi", "LMi", "Crt",
    "Lyr", "Cir", "Ara", "Sco", "Tri", "Leo", "Nor", "Sct", "Cae", "Scl", "Gru", "Men", "Lib",
    "Lac", "Hor", "Vol", "Pup", "Mus", "Cyg", "Oct", "Col", "Aps", "Gem", "Peg", "Ser", "Oph",
    "Her", "Per", "Vel", "Tel", "Phe", "Ant", "Aqr", "Hyi", "Cru", "PsA", "CrA", "TrA", "Sge",
    "Cap", "Lyn", "Pyx", "Dra", "Car", "CVn", "Ret", "For", "Sex", "Aql",
];
const GREEK: [char; 24] = [
    'α', 'β', 'γ', 'δ', 'ε', 'ζ', 'η', 'θ', 'ι', 'κ', 'λ', 'μ', 'ν', 'ξ', 'ο', 'π', 'ρ', 'σ', 'τ',
    'υ', 'φ', 'χ', 'ψ', 'ω',
];
impl LayerOptions {
    pub fn point_style(&self, mag: f32) -> (f32, f32) {
        let opacity = (1.2_f64 - f64::from(mag) * 0.07).clamp(0.3, 1.) / f64::from(self.brightness);
        let size = (2.8 - f64::from(mag) * 0.32).clamp(0.55, 3.6);
        (
            (size * opacity.max(1.).sqrt()).min(32.) as f32,
            opacity.min(1.) as f32,
        )
    }
    pub fn name(&self, c: &LayersCatalog, s: &Star) -> String {
        let mut parts = vec![];
        if self.proper_names && self.includes_name(s.hip, c) {
            if let Some(n) = c.names.get(&s.hip) {
                parts.push(n.clone());
            }
        }
        let mut id = String::new();
        if self.flamsteed && s.flamsteed_number >= 0 {
            id.push_str(&s.flamsteed_number.to_string());
        }
        if self.bayer {
            if let Some(&g) = GREEK.get(s.bayer_id as usize) {
                id.push(g);
                if s.bayer_subscript >= 0 {
                    let sup = ['⁰', '¹', '²', '³', '⁴', '⁵', '⁶', '⁷', '⁸', '⁹'];
                    for d in s.bayer_subscript.to_string().bytes() {
                        id.push(sup[(d - b'0') as usize]);
                    }
                }
            }
        }
        if !id.is_empty() {
            if let Some(abbr) = ABBR.get(s.constellation_id as usize) {
                id.push_str(abbr);
            }
            if (33..=126).contains(&s.ads_component) {
                id.push(s.ads_component as u8 as char);
            }
            parts.push(id);
        }
        if self.hip {
            parts.push(format!("HIP {}", s.hip));
        }
        parts.join(" · ")
    }
}
