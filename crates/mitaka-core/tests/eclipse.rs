use mitaka_core::eclipse::*;
#[test]
fn disk_overlap_limits_and_shadow_direction() {
    let sun = Sphere {
        center_au: [0.; 3],
        radius_au: 2.,
    };
    let o = Sphere {
        center_au: [10., 0., 0.],
        radius_au: 1.,
    };
    let rec = Sphere {
        center_au: [15., 0., 0.],
        radius_au: 0.5,
    };
    for (r, d, f) in [(0., 0., 0.), (1., 0., 1.), (0.5, 0., 0.25), (1., 2., 0.)] {
        assert_eq!(obscured_fraction(r, d).unwrap(), f);
    }
    assert!(
        (obscured_fraction(1., 1.).unwrap()
            - (2. / 3. - 3f64.sqrt() / (2. * std::f64::consts::PI)))
            .abs()
            < 1e-14
    );
    assert_eq!(
        shadow_axis_hit(sun, o, rec).unwrap().unwrap(),
        [14.5, 0., 0.]
    );
    let cones = shadow_cones(sun, o, rec).unwrap().unwrap();
    assert_eq!(cones.umbra.apex_au, [20., 0., 0.]);
    assert_eq!(
        surface_sample(sun, o, rec.center_au)
            .unwrap()
            .obscured_fraction,
        1.
    );
    assert_eq!(
        shadow_multiplier(0.5, [0., 0.5, 1.]).unwrap(),
        [0.5, 0.75, 1.]
    );
    assert!(obscured_fraction(-1., 1.).is_err());
    assert!(surface_sample(sun, o, o.center_au).is_err());
}
