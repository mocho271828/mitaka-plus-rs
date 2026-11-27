use mitaka_core::{catalog::CatalogLimits, galaxy::*};
use std::path::PathBuf;
#[test]
fn raw_galaxy_rows_keep_nan_and_source_identity() {
    let limits = CatalogLimits::default();
    let mut raw = b"A\0B\0C\0D\0E\0F\0".to_vec();
    for n in [0f32, 90., 125., f32::NAN, 0.1, 0., -0.] {
        raw.extend(n.to_le_bytes());
    }
    for n in 1..40 {
        assert!(parse_globular_clusters(raw[..n].to_vec(), limits).is_err());
    }
    let data = parse_globular_clusters(raw.repeat(2), limits).unwrap();
    assert_eq!(data.records.len(), 2);
    assert_eq!(data.raw, raw.repeat(2));
    assert!(data.records[0].core_radius_pc.is_nan());
    assert_eq!(data.records[0].vr.to_bits(), (-0f32).to_bits());
    let position = evaluate_position_pc(0., 90., 125.).position.unwrap();
    assert_eq!(position.equatorial[2], 125.);
    assert!(evaluate_position_pc(f32::NAN, 100., -1.).position.is_none());
    assert_eq!(
        evaluate_position_pc(0., 0., 0.).issues,
        [PositionIssue::ZeroDistance]
    );
    let names = parse_names(
        b"%hello\r\n\nN1 *\x83\x5c\r\nN1  two\nX\0Y\nKEY ".to_vec(),
        limits,
    )
    .unwrap();
    assert_eq!(names.records.len(), 6);
    assert_eq!(names.records[2].kind, NameKind::Entry);
    assert!(names.records[2].featured);
    assert_eq!(names.records[2].payload, b"\x83\x5c\r");
    assert_eq!(names.records[3].source_row, 4);
    assert!(matches!(names.records[4].kind, NameKind::Unresolved(_)));
}
#[test]
fn maps_and_clouds_validate_geometry_not_raw_values() {
    let limits = CatalogLimits::default();
    let mut raw = Vec::new();
    for (w, h, values) in [(2i32, 1i32, vec![f32::NAN, -1.]), (1, 1, vec![0.5])] {
        raw.extend(w.to_le_bytes());
        raw.extend(h.to_le_bytes());
        for v in values {
            raw.extend(v.to_le_bytes());
        }
    }
    for count in [1i32, 0, 1, 0] {
        raw.extend(count.to_le_bytes());
        for _ in 0..count {
            for v in [1f32, f32::INFINITY, -0.] {
                raw.extend(v.to_le_bytes());
            }
        }
    }
    let m = OurGalaxyModel::from_bytes(raw.clone(), limits).unwrap();
    assert_eq!(m.maps[0].nonfinite_count, 1);
    assert_eq!(m.maps[0].outside_unit_interval_count, 1);
    assert_eq!(m.clouds[2].nonfinite_count, 1);
    assert_eq!(m.raw, raw);
    for n in 0..raw.len() {
        assert!(OurGalaxyModel::from_bytes(raw[..n].to_vec(), limits).is_err());
    }
    assert!(
        OurGalaxyModel::from_bytes(
            raw.clone(),
            CatalogLimits {
                max_records: 2,
                ..limits
            }
        )
        .is_err()
    );
    raw.push(0);
    assert!(OurGalaxyModel::from_bytes(raw, limits).is_err());
}
#[test]
#[ignore = "requires an explicitly supplied Plus 1.5.4 data directory"]
fn plus_galaxy_real_data() {
    let dir = PathBuf::from(std::env::var_os("MITAKA_DATA_ROOT").expect("set MITAKA_DATA_ROOT"));
    let clusters = load_globular_clusters(&dir.join("globular_cluster.dat")).unwrap();
    let galaxies = load_nearby_galaxies(&dir.join("NBG.dat")).unwrap();
    let model = OurGalaxyModel::load(&dir.join("ourgalaxy_model.dat")).unwrap();
    assert_eq!(clusters.records.len(), 139);
    assert_eq!(galaxies.records.len(), 2371);
    assert!(model.maps.iter().all(|m| m.width == 512 && m.height == 512));
    assert!(
        !load_names(&dir.join("nearby_galaxy_names_jp.dat"))
            .unwrap()
            .records
            .is_empty()
    );
}
