use mitaka_core::{astronomy::J2000, satellite::*};
fn record(plane: u8) -> RotationInput {
    RotationInput {
        name_bytes: b"fixture".to_vec(),
        reference_plane: plane,
        elements: Some(SatelliteElements {
            epoch_jd: J2000,
            semi_major_axis_km: 1000.,
            eccentricity: 0.,
            mean_anomaly_deg: 0.,
            mean_motion_deg_per_day: 90.,
            argument_of_periapsis_deg: 0.,
            inclination_deg: 0.,
            raw_node_deg: 0.,
            periapsis_period_years: 0.,
            node_period_years: 0.,
            laplace_pole_deg: Some([270., 90.]),
        }),
        correct_mars_node: false,
        charon_index: None,
        parent_pole: None,
    }
}
#[test]
fn explicit_dependency_dates_and_rotation_shapes() {
    let e = record(b'E');
    let i = record(b'I');
    let mut c = record(b'C');
    c.charon_index = Some(1);
    let data = [c.clone(), i.clone(), e.clone()];
    assert_eq!(
        satellite_rotation(&data, 0, J2000).unwrap().axes,
        satellite_rotation(&data, 1, J2000).unwrap().axes
    );
    c.charon_index = Some(0);
    assert!(satellite_rotation(&[c], 0, J2000).is_err());
    let mut q = record(b'Q');
    q.parent_pole = Some(legacy_parent_pole(2, J2000).unwrap());
    assert!(satellite_rotation(&[q.clone()], 0, J2000).is_ok());
    assert!(satellite_rotation(&[q], 0, J2000 + 1.).is_err());
    let mut l = record(b'L');
    assert!(satellite_rotation(&[l.clone()], 0, J2000).is_ok());
    l.elements.as_mut().unwrap().laplace_pole_deg = None;
    assert!(satellite_rotation(&[l], 0, J2000).is_err());
    assert!(satellite_rotation(&[record(b'?')], 0, J2000).is_err());
    let rot = satellite_rotation(std::slice::from_ref(&e), 0, J2000 + 1.).unwrap();
    let p = relative_position_km(e.elements.unwrap(), rot, J2000 + 1.).unwrap();
    assert!(p[0].abs() < 1e-10 && (p[1] - 1000.).abs() < 1e-10);
    assert!(relative_position_km(e.elements.unwrap(), rot, J2000).is_err());
    assert!(
        relative_position_km(
            e.elements.unwrap(),
            TimedRotation {
                jd: J2000,
                axes: [[0.; 3]; 3]
            },
            J2000
        )
        .is_err()
    );
    let mut moon = record(b'?');
    moon.name_bytes = b"Moon".to_vec();
    moon.elements = None;
    assert!(satellite_rotation(&[moon], 0, J2000).is_ok());
}
