use super::catalog::error;
use mitaka_core::{
    astronomy::j2000_equatorial_to_ecliptic,
    ephemeris::Cf96Ephemeris,
    lunar::{earth_center, lunar_position},
    satellite::{legacy_parent_pole, relative_position_km, satellite_rotation, RotationInput},
    satellite_catalog::{Dataset, Record},
};
use std::io;

const IDS: [&[u8]; 9] = [
    b"Mercury", b"Venus", b"Earth", b"Mars", b"Jupiter", b"Saturn", b"Uranus", b"Neptune", b"Pluto",
];
const KM_AU: f64 = 149_597_870.7;
fn registered(set: &Dataset, r: &Record) -> bool {
    r.orbit_source
        .and_then(|i| set.sources.get(i))
        .is_some_and(|s| s.read_complete && s.parsed && s.valid)
}
pub(super) fn position(
    set: &Dataset,
    source: usize,
    row: usize,
    system: &Cf96Ephemeris,
    jd: f64,
) -> io::Result<[f64; 3]> {
    let src = &set.sources[source];
    let s = &src.records[row];
    let parent = s
        .parent
        .and_then(|p| src.records.get(p))
        .filter(|p| p.block == "Planet")
        .ok_or_else(|| error("母天体の定義がありません"))?;
    let parent_no = IDS
        .iter()
        .position(|name| Some(*name) == parent.name())
        .ok_or_else(|| error("CF96 の母天体を特定できません"))?;
    let ordered_parents: Vec<_> = src.records.iter().filter(|p| p.block == "Planet").collect();
    if ordered_parents.len() != IDS.len()
        || !ordered_parents
            .iter()
            .zip(IDS)
            .all(|(r, id)| r.name() == Some(id) && registered(set, r))
    {
        return Err(error("母天体の登録順と軌道資料を確認できません"));
    }
    if !registered(set, s) {
        return Err(error("衛星に対応する軌道資料を読めません"));
    }
    let base = system.state(parent_no, jd)?.position_au;
    if parent_no == 2 && s.name() == Some(b"Moon") {
        let moon = lunar_position(jd)?;
        let earth = earth_center(base, moon);
        return j2000_equatorial_to_ecliptic(std::array::from_fn(|i| earth[i] + moon[i]));
    }
    // 冥王星系では原版の母天体と衛星が同じ重心補正を受ける。
    // この独立モデルには系質量比を渡していない。
    if parent_no == 8 {
        return Err(error("冥王星系の重心補正を確定できません"));
    }
    let orbit = &set.sources[s.orbit_source.unwrap()].records[0];
    let elements = orbit
        .elements()
        .ok_or_else(|| error("衛星の軌道要素が不足しています"))?;
    let plane = orbit
        .field("RefPlane")
        .and_then(|f| match f.value_bytes.as_slice() {
            [v] => Some(*v),
            _ => None,
        })
        .ok_or_else(|| error("軌道基準面が不明です"))?;
    let input = RotationInput {
        name_bytes: s.name().unwrap_or_default().to_vec(),
        reference_plane: plane,
        elements: Some(elements),
        correct_mars_node: false,
        charon_index: None,
        parent_pole: if plane == b'Q' {
            Some(legacy_parent_pole(parent_no, jd)?)
        } else {
            None
        },
    };
    let axes = satellite_rotation(&[input], 0, jd)?;
    let delta = relative_position_km(elements, axes, jd)?;
    let center = if parent_no == 2 {
        earth_center(base, lunar_position(jd)?)
    } else {
        base
    };
    let center = j2000_equatorial_to_ecliptic(center)?;
    Ok(std::array::from_fn(|i| center[i] + delta[i] / KM_AU))
}
