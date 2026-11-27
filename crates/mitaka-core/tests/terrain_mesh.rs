use mitaka_core::terrain::{Amplification, Bounds, Terrain, unit_direction};
use std::{fs, path::PathBuf};
#[test]
fn amplification_stages_and_lod_use_source_ordinals() {
    for (stage, factor) in [1., 2., 5., 10., 20.].into_iter().enumerate() {
        let mode = Amplification::from_stage(stage as u64 + 1).unwrap();
        assert_eq!(mode.factor(), factor);
    }
    assert!(Amplification::from_stage(0).is_err());
    assert!(Amplification::from_stage(6).is_err());
    let patch = Bounds {
        west: 20.,
        east: 22.,
        north: 12.,
        south: 10.,
        levels: 4,
    };
    assert_eq!(patch.level(unit_direction(21., 11.), false).unwrap(), 3);
    assert_eq!(patch.level(unit_direction(21., 11.), true).unwrap(), 2);
    assert_eq!(patch.level([2000., 0., 0.], false).unwrap(), 0);
}
#[test]
fn halo_derivatives_cell_diagonals_and_signed_displacement() {
    let path = std::env::temp_dir().join(format!("mitaka-raw-geometry-{}.dat", std::process::id()));
    let mut bytes = Vec::new();
    for n in [2f32, 1_000_000.] {
        bytes.extend(n.to_le_bytes());
    }
    for n in [1i32, 1] {
        bytes.extend(n.to_le_bytes());
    }
    for v in [-2f32, 2., 2., -2.] {
        bytes.extend(v.to_le_bytes());
    }
    for n in [0i32, 7, 2, 2] {
        bytes.extend(n.to_le_bytes());
    }
    for y in -1..=3i16 {
        for x in -1..=3i16 {
            bytes.extend((x + y - 2).to_le_bytes());
        }
    }
    fs::write(&path, bytes).unwrap();
    let mut file = Terrain::open(&path).unwrap();
    let mesh = file.grid(0, 0).unwrap().mesh(Amplification::Ten).unwrap();
    assert_eq!(mesh.triangles().next(), Some([0, 3, 1]));
    assert_eq!(mesh.triangles().nth(1), Some([1, 3, 4]));
    assert_eq!(mesh.vertices[0].elevation_m, -4.);
    assert!((mesh.radial_intersection(unit_direction(0., 0.)).unwrap() - 1.).abs() < 1e-12);
    assert_eq!(file.elevation_m(0., 0.).unwrap(), 0.);
    assert!(file.grid(0, 1).is_err());
    drop(file);
    fs::remove_file(path).unwrap();
}
#[test]
#[ignore = "requires Mitaka Plus data"]
fn both_planets_keep_source_grid_order_and_signed_heights() {
    let root = PathBuf::from(std::env::var_os("MITAKA_DATA_ROOT").unwrap());
    for body in ["earth", "mars"] {
        let mut t = Terrain::open(&root.join(format!("{body}_topo/{body}_topo.dat"))).unwrap();
        assert!(t.patch_count() > 100);
        let (idx, lon) = t.locate(139.76, 35.68).unwrap();
        let highest = t.bounds(idx).unwrap().levels - 1;
        let bilinear = t
            .grid(idx, highest)
            .unwrap()
            .elevation_m(lon, 35.68)
            .unwrap();
        assert_eq!(bilinear, t.elevation_m(139.76, 35.68).unwrap());
    }
}
