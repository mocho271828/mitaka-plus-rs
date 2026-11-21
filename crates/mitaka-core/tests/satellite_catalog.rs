use mitaka_core::satellite_catalog::*;
use std::{fs, path::PathBuf};
#[test]
fn parser_isolates_invalid_sources_and_preserves_ambiguous_fields() {
    let temp =
        std::env::temp_dir().join(format!("mitaka_satellite_catalog_{}", std::process::id()));
    let data = temp.join("data");
    fs::create_dir_all(data.join("orbits")).unwrap();
    fs::write(
        data.join("ssystem.dat"),
        b"Ssystem\n{\nPlanet\n{\nName: P\nOrbit: ./data/orbits/p.dat\n}\n}\n",
    )
    .unwrap();
    fs::write(data.join("ssystem_jp.dat"), b"Planet\n{\nName: broken\n").unwrap();
    fs::write(
        data.join("orbits/p.dat"),
        b"a: 1\na: 2\nExtra: raw\nRefPlane: X",
    )
    .unwrap();
    fs::write(data.join("orbits/other.dat"), b"bad field").unwrap();
    let d = Dataset::load(&data, Limits::default());
    let s = d.sources.iter().find(|s| s.key == "ssystem.dat").unwrap();
    assert!(s.parsed && s.valid);
    assert_eq!(s.records[1].parent, Some(0));
    assert_eq!(s.records[1].name(), Some(b"P".as_slice()));
    let p = &d.sources[s.records[1].orbit_source.unwrap()];
    assert_eq!(p.key, "orbits/p.dat");
    assert!(p.parsed && !p.valid);
    assert!(p.records[0].scalar("a").is_none());
    assert!(d.sources.iter().filter(|s| !s.parsed).count() == 2);
    assert!(
        d.diagnostics
            .iter()
            .any(|d| d.message.contains("duplicate"))
    );
    std::fs::remove_dir_all(temp).unwrap();
}
#[test]
#[ignore = "requires an explicitly supplied Plus 1.5.4 data directory"]
fn preserves_all_bilingual_source_blocks() {
    let path = PathBuf::from(std::env::var_os("MITAKA_DATA_ROOT").unwrap());
    let d = Dataset::load(&path, Limits::default());
    assert!(d.diagnostics.is_empty(), "{:?}", d.diagnostics);
    assert_eq!(d.sources.len(), 179);
    let system: Vec<_> = d
        .sources
        .iter()
        .filter(|s| s.kind == SourceKind::System)
        .flat_map(|s| s.records.iter())
        .collect();
    assert_eq!(
        system.iter().filter(|r| r.block == "Satellite").count(),
        340
    );
    assert_eq!(system.iter().filter(|r| r.block == "Planet").count(), 18);
    assert_eq!(
        system.iter().filter(|r| r.orbit_source.is_some()).count(),
        358
    );
}
