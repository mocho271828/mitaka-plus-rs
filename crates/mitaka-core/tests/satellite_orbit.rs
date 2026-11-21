//! C++ f9fe624 satellite_orbit_geometry() の日心黄道 AU。各点は元版の256分割での添字。
use mitaka_core::{
    ephemeris::Cf96Ephemeris,
    satellite_catalog::{Dataset, Limits, SourceKind},
    satellite_model::Model,
};
use std::{path::PathBuf, sync::Arc};

#[test]
#[ignore = "requires Mitaka Plus data in MITAKA_DATA_ROOT"]
fn native_instantaneous_orbits_and_unavailable_rows() {
    let root = PathBuf::from(std::env::var_os("MITAKA_DATA_ROOT").unwrap());
    let d = Dataset::load(&root, Limits::default());
    let eph = Cf96Ephemeris::load(&root.join("cf96.dat")).unwrap();
    for (source, s) in d
        .sources
        .iter()
        .enumerate()
        .filter(|(_, s)| s.kind == SourceKind::System)
    {
        let model = Model::from_source(&d, source).unwrap();
        for (jd, ordinal, index, expected) in [
            (
                2451545.,
                7,
                64,
                [
                    1.3906628241905559,
                    -0.01344968706204655,
                    -0.03446788010513628,
                ],
            ),
            (
                2451546.,
                178,
                128,
                [-9.87232555995679, -27.960283472465875, 5.849816518194761],
            ),
            (
                2460000.5,
                180,
                0,
                [16.33358791308526, -30.59749133083331, -1.4477176231684836],
            ),
        ] {
            let snap = model.evaluate(&eph, jd).unwrap();
            let n = s
                .records
                .iter()
                .filter(|r| r.block == "Satellite" && snap.orbit_au(r.ordinal).is_ok())
                .count();
            assert_eq!(n, 164, "{}", s.key);
            assert!(snap.position_au(5).is_ok() && snap.orbit_au(5).is_err());
            let points = snap.orbit_au(ordinal).unwrap();
            assert!(Arc::ptr_eq(&points, &snap.orbit_au(ordinal).unwrap()));
            assert_eq!(points.len(), 257);
            assert_eq!(points[0], points[256]);
            for (v, target) in points[index].iter().zip(expected) {
                assert!(
                    (v - target).abs() < 2e-12,
                    "{jd} {}:{ordinal}:{index} {v} {target}",
                    s.key
                );
            }
        }
    }
}
