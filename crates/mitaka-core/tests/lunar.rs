use mitaka_core::{
    astronomy::{J2000, j2000_equatorial_to_ecliptic},
    lunar::*,
};
fn max_abs(a: [f64; 3], b: [f64; 3]) -> f64 {
    a.into_iter()
        .zip(b)
        .map(|(a, b)| {
            let difference = (a - b).abs();
            assert!(
                difference.is_finite(),
                "non-finite lunar reference difference: {a} vs {b}"
            );
            difference
        })
        .fold(0., f64::max)
}

#[test]
#[should_panic(expected = "non-finite lunar reference difference")]
fn reference_comparison_rejects_a_nan_component() {
    max_abs([1., f64::NAN, 2.], [1., 0., 2.]);
}

#[test]
#[allow(
    clippy::excessive_precision,
    reason = "Keep the published C++ reference digits verbatim"
)]
fn official_reference_positions_on_both_compiler_profiles() {
    // f9fe624 tests/test_lunar.cpp and docs/core/LUNAR_PROVENANCE.md: the legacy
    // series has two slightly different floating-point profiles on arm64 and x86.
    let fixtures = [
        (
            2415020.5,
            [
                [
                    0.00016353511842929789,
                    -0.0022715366457242976,
                    -0.00093358837226923503,
                ],
                [
                    0.00016353511843153116,
                    -0.0022715366457242377,
                    -0.00093358837226900095,
                ],
            ],
        ),
        (
            J2000,
            [
                [
                    -0.0019487158434038107,
                    -0.0017826753996118328,
                    -0.00050866105949668491,
                ],
                [
                    -0.001948715843403804,
                    -0.001782675399611827,
                    -0.00050866105949668209,
                ],
            ],
        ),
        (
            2461342.5,
            [
                [
                    0.00068081714196286433,
                    0.0020562891351227899,
                    0.0011145543535771747,
                ],
                [
                    0.00068081714196285294,
                    0.0020562891351227543,
                    0.0011145543535771561,
                ],
            ],
        ),
        (
            2487980.5,
            [
                [
                    0.0013329808753983834,
                    0.0020144422400323317,
                    0.0011136831814718244,
                ],
                [
                    0.0013329808753899234,
                    0.0020144422400365796,
                    0.0011136831814736795,
                ],
            ],
        ),
    ];
    for (jd, [a, b]) in fixtures {
        let actual = lunar_position(jd).unwrap();
        let tolerance = if jd == J2000 { 2e-17 } else { 2e-15 };
        assert!(max_abs(actual, a).min(max_abs(actual, b)) <= tolerance);
    }
    let expected_x = [
        0.7842270520919169,
        0.61987147459511771,
        0.027336527769559568,
    ];
    assert!(max_abs(lunar_orientation(J2000).unwrap()[0], expected_x) < 2e-14);
}
#[test]
fn evaluated_range_has_orthonormal_axes_and_near_side_facing_earth() {
    fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
        a.into_iter().zip(b).map(|(a, b)| a * b).sum()
    }
    for i in 0..=1000 {
        let jd = LUNAR_MINIMUM_JD + (LUNAR_MAXIMUM_JD - LUNAR_MINIMUM_JD) * (i as f64 / 1000.);
        let [x, y, z] = lunar_orientation(jd).unwrap();
        let p = j2000_equatorial_to_ecliptic(lunar_position(jd).unwrap()).unwrap();
        assert!((0.0022..0.0029).contains(&dot(p, p).sqrt()));
        assert!(-dot(p, x) / dot(p, p).sqrt() > 0.94);
        for axis in [x, y, z] {
            assert!((dot(axis, axis) - 1.).abs() < 2e-15);
        }
        for (a, b) in [(x, y), (y, z), (z, x)] {
            assert!(dot(a, b).abs() < 2e-15);
        }
    }
    for jd in [LUNAR_MINIMUM_JD - 1., LUNAR_MAXIMUM_JD + 1., f64::NAN] {
        assert!(lunar_position(jd).is_err() && lunar_orientation(jd).is_err());
    }
    assert_eq!(
        earth_center([1.; 3], [EARTH_MOON_MASS_RATIO + 1.; 3]),
        [0.; 3]
    );
}
