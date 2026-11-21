use mitaka_core::{astronomy::J2000, rotation::planet_orientation};
#[test]
fn spin_directions_match_planet_coefficients_and_axes_are_orthonormal() {
    let rates = [
        6.1385025,
        -1.4813688,
        360.985612288,
        350.89198226,
        870.536,
        810.7939024,
        -501.1600928,
        536.3128492,
        56.3625225,
    ];
    fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
        a.into_iter().zip(b).map(|(a, b)| a * b).sum()
    }
    for jd in [J2000 - 36525., J2000, J2000 + 36525.] {
        for (i, rate) in rates.into_iter().enumerate() {
            let before = planet_orientation(i, jd).unwrap();
            let after = planet_orientation(i, jd + 0.02).unwrap();
            let measured = dot(after[0], before[1])
                .atan2(dot(after[0], before[0]))
                .to_degrees()
                / ((jd + 0.02) - jd);
            assert!((rate - measured).abs() < 0.002);
            for axis in before {
                assert!((dot(axis, axis) - 1.).abs() < 2e-15);
            }
            assert!(dot(before[0], before[2]).abs() < 2e-15);
        }
    }
    assert!(planet_orientation(9, J2000).is_err());
}
