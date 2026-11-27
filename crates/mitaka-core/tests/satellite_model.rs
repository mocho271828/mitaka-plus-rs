use mitaka_core::{
    ephemeris::Cf96Ephemeris,
    satellite_catalog::{Dataset, Limits, Source, SourceKind},
    satellite_model::Model,
};
use std::path::PathBuf;
#[test]
fn missing_registration_does_not_fall_back_to_planet_names() {
    let data = Dataset {
        sources: vec![Source {
            key: "ssystem.dat".into(),
            kind: SourceKind::System,
            raw: Vec::new(),
            read_complete: true,
            parsed: true,
            valid: true,
            records: Vec::new(),
        }],
        diagnostics: Vec::new(),
    };
    assert!(Model::from_source(&data, 0).is_err());
    assert!(Model::from_source(&data, 1).is_err());
}
#[test]
#[ignore = "requires Mitaka Plus data"]
fn isolated_registrations_and_native_reference_positions() {
    let data = PathBuf::from(std::env::var_os("MITAKA_DATA_ROOT").unwrap());
    let d = Dataset::load(&data, Limits::default());
    let eph = Cf96Ephemeris::load(&data.join("cf96.dat")).unwrap();
    for (source, system) in d
        .sources
        .iter()
        .enumerate()
        .filter(|(_, s)| s.kind == SourceKind::System)
    {
        let model = Model::from_source(&d, source).unwrap();
        for (jd, expected) in [
            (
                2451545.,
                [
                    [
                        -0.1790838116652375,
                        0.9654037790217513,
                        0.00023832715795073236,
                    ],
                    [
                        1.3906677769465188,
                        -0.013455382970523545,
                        -0.03447830263639894,
                    ],
                    [16.81068626485301, -24.992484164403983, 0.1290253430677553],
                    [-9.875358104262752, -27.95889987110901, 5.850519429008777],
                    [-9.87540769512176, -27.958573617820207, 5.850220003997397],
                    [-9.875297556872644, -27.958908986391453, 5.850847778608959],
                ],
            ),
            (
                2460000.5,
                [
                    [
                        -0.9006719374396037,
                        0.4074305044534855,
                        -0.000005340800198541081,
                    ],
                    [-0.6588823254060567, 1.482217450725697, 0.047277341598385585],
                    [29.774417872103815, -2.7705716321932856, -0.6303224061863031],
                    [16.333535432484307, -30.597357721163508, -1.44802658657209],
                    [16.333544638373752, -30.597118450117158, -1.4479875100080979],
                    [16.333463374568165, -30.597747659576775, -1.4483987356311152],
                ],
            ),
        ] {
            let snap = model.evaluate(&eph, jd).unwrap();
            assert_eq!(snap.pair_corrected_ordinals.len(), 4);
            assert_eq!(
                system
                    .records
                    .iter()
                    .filter(|r| r.block == "Satellite" && snap.position_au(r.ordinal).is_ok())
                    .count(),
                165
            );
            for (ordinal, vector) in [5, 7, 170, 178, 179, 180].into_iter().zip(expected) {
                for (v, e) in snap.position_au(ordinal).unwrap().into_iter().zip(vector) {
                    assert!((v - e).abs() < 2e-12, "{jd} {ordinal}: {v} != {e}");
                }
            }
        }
        assert!(model.evaluate(&eph, 2487981.).is_err());
    }
}
