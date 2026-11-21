use mitaka_core::terrain::Terrain;
use std::{fs, io};

#[test]
fn reads_signed_grid_and_rejects_truncation() -> io::Result<()> {
    let path = std::env::temp_dir().join(format!(
        "mitaka-terrain-{}-synthetic.dat",
        std::process::id()
    ));
    let mut bytes = Vec::new();
    for scalar in [1f32, 3000000.0] {
        bytes.extend(scalar.to_le_bytes());
    }
    for n in [1i32, 1] {
        bytes.extend(n.to_le_bytes());
    }
    for v in [0f32, 360.0, 90.0, -90.0] {
        bytes.extend(v.to_le_bytes());
    }
    for n in [0i32, 99, 1, 1] {
        bytes.extend(n.to_le_bytes());
    }
    for value in [0i16, 0, 0, 0, 0, -300, -100, 0, 0, -100, 100, 0, 0, 0, 0, 0] {
        bytes.extend(value.to_le_bytes());
    }
    fs::write(&path, &bytes)?;
    {
        let mut grid = Terrain::open(&path)?;
        assert_eq!(grid.radius_m(), 3_000_000.0);
        assert_eq!(grid.elevation_m(180.0, 0.0)?, -100.);
        assert_eq!(grid.elevation_m(-180.0, 0.0)?, -100.);
        assert_eq!(grid.elevation_m(360.0, -90.0)?, 100.);
        assert!(grid.elevation_m(12., f64::NAN).is_err());
    }
    for limit in [0, 15, 17, 35, 46, bytes.len() - 1] {
        fs::write(&path, &bytes[..limit])?;
        assert!(Terrain::open(&path).is_err());
    }
    bytes.push(0);
    fs::write(&path, bytes)?;
    assert!(Terrain::open(&path).is_err());
    fs::remove_file(path)
}
