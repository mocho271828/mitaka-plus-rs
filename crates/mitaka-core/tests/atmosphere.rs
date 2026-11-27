use mitaka_core::{atmosphere::*, catalog::CatalogLimits};
#[test]
fn emission_grid_single_and_multiple_spectra_and_clipping() {
    let p = Parameters {
        ground_radius_m: 6378136.6,
        outer_radius_m: 6442136.6,
        scale_height_m: 8000.,
        surface_number_density_per_m3: 2.69e25,
        incident_intensity: 1.,
    };
    let ray = Ray {
        observer_position_m: [0., 0., 6380000.],
        view_direction: [0., 0., 1.],
        sun_direction: [0., 0., 1.],
        maximum_distance_m: None,
    };
    let mut b = vec![];
    b.extend(2i32.to_le_bytes());
    b.extend(3i32.to_le_bytes());
    for v in [6378000., 6442000., 8000., 2.5469e25, std::f64::consts::PI] {
        b.extend(v.to_le_bytes());
    }
    for i in 1..19 {
        b.extend((i as f32 * 1e-8).to_le_bytes());
    }
    let l = CatalogLimits::default();
    let map = EmissionMap::from_bytes(&b, l).unwrap();
    for i in 0..b.len() {
        assert!(EmissionMap::from_bytes(&b[..i], l).is_err());
    }
    assert_eq!(map.dimensions(), [2, 3]);
    let one = single_scattering(p, ray, 14).unwrap();
    assert!(one[2] > one[1] && one[1] > one[0] && one[0] > 0.);
    let multi = multiple_scattering(&map, ray).unwrap();
    assert!(multi.iter().all(|&v| v > 0.));
    let clipped = Ray {
        maximum_distance_m: Some(0.),
        ..ray
    };
    assert_eq!(single_scattering(p, clipped, 14).unwrap(), [0.; 3]);
    assert_eq!(multiple_scattering(&map, clipped).unwrap(), [0.; 3]);
    assert_eq!(
        single_scattering(
            Parameters {
                incident_intensity: 2.,
                ..p
            },
            ray,
            14
        )
        .unwrap(),
        one.map(|v| 2. * v)
    );
    assert!(
        single_scattering(
            p,
            Ray {
                view_direction: [0.; 3],
                ..ray
            },
            14
        )
        .is_err()
    );
    assert_eq!(
        compose_display([0.1, 0.2, 0.3], [0.02, 0.03, 0.04], 2.).unwrap(),
        ([0.1 + 0.04, 0.2 + 0.06, 0.3 + 0.08], 1.)
    );
}
#[test]
#[ignore = "requires an explicitly supplied Plus emmap.dat"]
fn real_emission_map() {
    let path = std::path::PathBuf::from(std::env::var_os("MITAKA_DATA_ROOT").unwrap());
    let m = EmissionMap::load(&path.join("emmap.dat")).unwrap();
    assert!(m.dimensions().into_iter().all(|n| n >= 2));
}
