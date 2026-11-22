use mitaka_core::{astronomy::*, orbit::*};

#[test]
fn gregorian_j2000_leap_and_extreme_bounds() {
    let a = GregorianDateTime {
        year: 2000,
        month: 1,
        day: 1,
        hour: 12,
        minute: 0,
        second: 0.,
    };
    assert_eq!(julian_date(a).unwrap(), J2000);
    assert_eq!(gregorian_date_time(J2000).unwrap(), a);
    for jd in [37.5, 2451544.5, 5373484.49] {
        assert!((julian_date(gregorian_date_time(jd).unwrap()).unwrap() - jd).abs() < 1e-9);
    }
    assert!(gregorian_date_time(37.499).is_err());
    assert!(gregorian_date_time(5373484.5).is_err());
    assert!(gregorian_date_time(f64::NAN).is_err());
    let leap = GregorianDateTime {
        month: 2,
        day: 29,
        ..a
    };
    assert!(julian_date(leap).is_ok());
    assert!(julian_date(GregorianDateTime { year: 1900, ..leap }).is_err());
}
#[test]
fn cardinal_directions_and_inverse_across_precession_epochs() {
    let spin = 360. * 0.7790572732640;
    let f = ObservationFrame::new(0., 0., J2000).unwrap();
    for (ra, az, alt) in [
        (spin, 0., 90.),
        (spin + 90., 90., 0.),
        (spin - 90., 270., 0.),
    ] {
        let h = f.to_horizontal(ra, 0.).unwrap();
        assert!((h[0] - az).abs() < 1e-12 && (h[1] - alt).abs() < 1e-12);
    }
    for jd in [37.5, 2415020.5, J2000, 2460000.5, 5373484.49] {
        for lat in [-90., 35.68, 90.] {
            let f = ObservationFrame::new(lat, 139.76, jd).unwrap();
            for (ra, dec) in [(0., 90.), (123.456, -35.), (359.99, -89.99)] {
                let [az, alt] = f.to_horizontal(ra, dec).unwrap();
                let [r, d] = f.to_equatorial(az, alt).unwrap();
                let first = equatorial_direction(ra, dec).unwrap();
                let last = equatorial_direction(r, d).unwrap();
                for i in 0..3 {
                    assert!((first[i] - last[i]).abs() < 2e-14);
                }
            }
        }
    }
    assert!(ObservationFrame::new(91., 0., J2000).is_err());
    assert!(f.to_horizontal(0., 91.).is_err());
    assert!((greenwich_mean_sidereal_time_deg(J2000).unwrap() - 280.460622).abs() < 0.001);
}
#[test]
fn original_cpp_planet_states_and_hyperbolic_perihelion() {
    // core_probe planets 2451545, Mitaka_plus f9fe624, heliocentric ecliptic AU.
    for (i, refpos) in [
        (
            0,
            [
                -0.1296282841522638,
                -0.4474252251251998,
                -0.02461978023397542,
            ],
        ),
        (2, [-0.1779328497107384, 0.9670733854069763, 0.]),
        (
            8,
            [-10.23136197890866, -27.81766486245696, 5.944730456641488],
        ),
    ] {
        let state = mean_planet_state(i, J2000).unwrap();
        for (got, want) in state.position_au.iter().zip(refpos) {
            assert!((got - want).abs() < 1e-13);
        }
    }
    let orbit = OrbitalElements {
        epoch_jd: J2000,
        semi_major_axis_au: 2.,
        eccentricity: 1.5,
        mean_anomaly_deg: 0.,
        mean_motion_deg_per_day: 1.,
        ascending_node_deg: 0.,
        inclination_deg: 0.,
        argument_of_periapsis_deg: 0.,
    };
    assert_eq!(
        orbital_state(orbit, J2000).unwrap().position_au,
        [1., 0., 0.]
    );
    assert!(
        orbital_state(
            OrbitalElements {
                eccentricity: 1.,
                ..orbit
            },
            J2000
        )
        .is_err()
    );
    assert!(mean_planet_state(9, J2000).is_err());
}
