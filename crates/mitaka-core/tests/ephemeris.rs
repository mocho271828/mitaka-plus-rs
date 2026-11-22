use mitaka_core::ephemeris::Cf96Ephemeris;
fn fixture() -> Vec<u8> {
    let mut b = b"CF96".to_vec();
    b.extend(1i32.to_le_bytes());
    b.extend(2451544f64.to_le_bytes());
    b.extend(2f64.to_le_bytes());
    for n in [1i32, 1, 2, 1, 1] {
        b.extend(n.to_le_bytes());
    }
    b.extend(0.5f64.to_le_bytes());
    b.extend(1f64.to_le_bytes());
    for block in [1, 2] {
        for axis in 0..3 {
            b.extend(2i32.to_le_bytes());
            b.extend(((10 * block + axis) as f64 * 1e10).to_le_bytes());
            b.extend(2e10f64.to_le_bytes());
            b.extend(2i32.to_le_bytes());
            for (a, c) in [(3e10f64, 4e10f64), (5e10, 6e10)] {
                b.extend(1i32.to_le_bytes());
                b.extend(a.to_le_bytes());
                b.extend(c.to_le_bytes());
            }
        }
    }
    b
}
#[test]
fn polynomial_and_periodic_terms_and_derivatives() {
    let eph = Cf96Ephemeris::from_bytes(&fixture()).unwrap();
    assert_eq!(eph.planet_count(), 1);
    assert_eq!(eph.time_range(0).unwrap(), [2451544., 2451548.]);
    let center = eph.state(0, 2451545.).unwrap();
    assert_eq!(center.position_au, [13., 14., 15.]);
    assert_eq!(center.velocity_au_per_day, [9.; 3]);
    let (s1, c1) = (-0.5f64).sin_cos();
    let (s2, c2) = (-1f64).sin_cos();
    assert!(
        (eph.state(0, 2451546.).unwrap().position_au[0]
            - (18. + 3. * c1 + 4. * s1 - 5. * c2 - 6. * s2))
            .abs()
            < 1e-12
    );
    for jd in [2451544., 2451548.] {
        assert!(eph.state(0, jd).is_ok());
    }
    for jd in [2451543., 2451549., f64::NAN] {
        assert!(eph.state(0, jd).is_err());
    }
    assert!(eph.state(1, 2451545.).is_err());
}
#[test]
fn malformed_dimensions_and_trailing_bytes_rejected() {
    let mut b = fixture();
    for end in 0..b.len() {
        assert!(Cf96Ephemeris::from_bytes(&b[..end]).is_err());
    }
    b.push(0);
    assert!(Cf96Ephemeris::from_bytes(&b).is_err());
    b.pop();
    b[60] = 3;
    assert!(Cf96Ephemeris::from_bytes(&b).is_err());
    let mut b = fixture();
    b[8..16].copy_from_slice(&f64::NAN.to_le_bytes());
    assert!(Cf96Ephemeris::from_bytes(&b).is_err());
}
