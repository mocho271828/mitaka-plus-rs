use mitaka_core::{
    galaxy::{GalaxyMap, OurGalaxyModel, PointCloud},
    galaxy_radiative::{RadiativeModel, Ray, point_extinction},
};
use std::path::PathBuf;
fn constant(v: f32) -> OurGalaxyModel {
    OurGalaxyModel {
        raw: vec![],
        maps: std::array::from_fn(|_| GalaxyMap {
            width: 512,
            height: 512,
            values: vec![v; 512 * 512],
            nonfinite_count: 0,
            outside_unit_interval_count: 0,
        }),
        clouds: std::array::from_fn(|_| PointCloud {
            positions_pc: vec![],
            nonfinite_count: 0,
        }),
    }
}
#[test]
fn misses_and_singularities_are_distinct() {
    let m = RadiativeModel::from_model(&constant(2.)).unwrap();
    let [thin, dust] = m.modulation(0., 0.).unwrap();
    assert_eq!(thin, 1.);
    assert!((dust - 0.0064).abs() < 1e-8);
    assert_eq!(
        m.trace([0., 0., 5000.], [0., 0., 1.], 40.).unwrap().steps,
        0
    );
    assert!(m.trace([0.; 3], [1., 0., 0.], 40.).is_err());
    assert!(m.trace([0.; 3], [0.; 3], 40.).is_err());
    assert_eq!(
        point_extinction([0.; 3], [0.; 3], 40.)
            .unwrap()
            .transmittance,
        [1.; 3]
    );
    assert!(
        !point_extinction([8000., 0., 0.], [-8000., 0., 0.], 40.)
            .unwrap()
            .visible
    );
    let ray = Ray {
        intensity: [0.1, 0.2, 0.3],
        transmittance: [0.2, 0.6, 0.9],
        ..Default::default()
    };
    for (r, e) in ray
        .compose([0.5; 3], 0.5)
        .unwrap()
        .into_iter()
        .zip([0.35, 0.5, 0.625])
    {
        assert!((r - e).abs() < 1e-6);
    }
    assert!(RadiativeModel::from_model(&constant(-1.)).is_err());
}
#[test]
#[ignore = "requires Mitaka Plus data"]
fn published_maps_match_native_fixed_rays() {
    let p =
        PathBuf::from(std::env::var_os("MITAKA_DATA_ROOT").unwrap()).join("ourgalaxy_model.dat");
    let model = RadiativeModel::from_model(&OurGalaxyModel::load(&p).unwrap()).unwrap();
    for (origin, expected, steps, point, point_steps) in [
        (
            [8000., 0., 50.],
            [0.49585435, 0.43666813, 0.34153944, 0.86164546],
            32,
            1.7977223,
            25,
        ),
        (
            [-4000., 2000., -10.],
            [0.50812525, 0.42454037, 0.32218927, 1.8528153],
            32,
            3.1034446,
            18,
        ),
        (
            [-5500., 1500., -6000.],
            [0., 0., 0., 0.],
            0,
            1.1231519e-7,
            6,
        ),
        (
            [-11000., -8000., 1000.],
            [0.0044430844, 0.004605368, 0.004365353, 0.0010931793],
            2,
            0.00006500666,
            7,
        ),
    ] {
        let ray = model.trace(origin, [-1., -1., 0.17], 40.).unwrap();
        assert_eq!(steps, ray.steps);
        for (&actual, exp) in ray
            .intensity
            .iter()
            .chain([&ray.optical_depth])
            .zip(expected)
        {
            assert!((actual - exp).abs() < 2e-5, "{actual} != {exp}");
        }
        let extinction = point_extinction(origin, [1000., -2000., 300.], 40.).unwrap();
        assert_eq!(extinction.steps, point_steps);
        assert!((extinction.optical_depth - point).abs() < 2e-5);
    }
}
