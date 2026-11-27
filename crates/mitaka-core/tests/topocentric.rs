use mitaka_core::{
    astronomy::{ObservationFrame, j2000_equatorial_to_ecliptic},
    topocentric::Observer,
};
#[test]
fn ellipsoid_radius_and_j2000_observer_match_native() {
    let radius = 6378.1366 / 149_597_870.7;
    for (latitude, norm) in [
        (0., radius),
        (90., radius * (1. - 1. / 298.257223563)),
        (-90., radius * (1. - 1. / 298.257223563)),
    ] {
        let p = Observer::new(latitude, 139.76, 2451545., [0.; 3])
            .unwrap()
            .offset_equatorial_au();
        assert!((p[0].hypot(p[1]).hypot(p[2]) - norm).abs() < 1e-17);
    }
    let site = Observer::new(35.68, 139.76, 2451545., [0.; 3]).unwrap();
    let p = site.offset_equatorial_au();
    for (v, c) in p.into_iter().zip([
        1.722002259877616e-5,
        3.009296007047683e-5,
        2.472901782794103e-5,
    ]) {
        assert!((v - c).abs() < 1e-17);
    }
    let frame = ObservationFrame::new(35.68, 139.76, 2451545.).unwrap();
    let [ra, dec] = frame.to_equatorial(90., 0.).unwrap();
    let d = mitaka_core::astronomy::equatorial_direction(ra, dec).unwrap();
    let obj = j2000_equatorial_to_ecliptic(std::array::from_fn(|i| p[i] + d[i] * 1e-5)).unwrap();
    let apparent = site.observe(obj).unwrap();
    assert!((apparent.horizontal_deg[0] - 90.).abs() < 1e-10);
    assert!(apparent.horizontal_deg[1].abs() < 1e-10);
    assert!((apparent.distance_au - 1e-5).abs() < 1e-18);
    assert!(site.observe([f64::NAN, 0., 0.]).is_err());
    assert!(Observer::new(91., 0., 2451545., [0.; 3]).is_err());
}
