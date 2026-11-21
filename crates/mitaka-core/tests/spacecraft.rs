use mitaka_core::spacecraft::{Dataset, Limits, Ppi, Pth, Sampling, SourceData};
use std::{fs, path::PathBuf};

fn bytes<const N: usize>(rows: &[[f64; N]]) -> Vec<u8> {
    rows.iter()
        .flatten()
        .flat_map(|v| v.to_le_bytes())
        .collect()
}
#[test]
fn pth_duplicate_and_clamp_order() {
    let b = bytes(&[
        [1., 1., 1., 1.],
        [2., 21., 2., 2.],
        [2., 22., 2., 2.],
        [2., 23., 2., 2.],
        [3., 3., 3., 3.],
    ]);
    let p = Pth::parse(&b, 5).unwrap();
    assert_eq!(p.duplicate_ordinals, [2, 3]);
    assert_eq!(p.position(2.).unwrap().first, 2);
    assert_eq!(p.position(0.).unwrap().sampling, Sampling::ClampedBefore);
    assert_eq!(p.position(4.).unwrap().sampling, Sampling::ClampedAfter);
    let v = p.position(2.5).unwrap();
    assert_eq!((v.first, v.second, v.u, v.position_au[0]), (3, 4, 0.5, 13.));
    assert!(
        Pth::parse(&b, 4).is_err()
            && p.position(f64::NAN).is_err()
            && Pth::parse(&b[1..], 5).is_err()
    );
    assert!(Pth::parse(&bytes(&[[2., 0., 0., 0.], [1., 0., 0., 0.]]), 2).is_err());
}
fn segment(t: f64, base: f64) -> [f64; 16] {
    let mut r = [0.; 16];
    for i in 0..4 {
        let x = i as f64 + t;
        r[i] = x;
        r[4 + i] = base + x * x * x;
        r[8 + i] = base - x;
        r[12 + i] = t;
    }
    r
}
#[test]
fn ppi_cubic_gaps_and_touching_segments() {
    let p = Ppi::parse(
        &bytes(&[segment(0., 0.), segment(3., 10.), segment(7., 20.)]),
        3,
    )
    .unwrap();
    assert_eq!(p.native_position_km(1.5).unwrap(), [3.375, -1.5, 0.]);
    assert_eq!(p.native_position_km(3.).unwrap(), [27., -3., 0.]);
    assert!(p.native_position_km(6.5).is_err() && p.native_position_km(-1.).is_err());
    assert!(Ppi::parse(&bytes(&[segment(0., 0.), segment(2., 0.)]), 2).is_err());
}
#[test]
fn resolve_only_local_references_and_retain_registration_order() {
    let root = std::env::temp_dir().join(format!("mitaka-rs-spacecraft-{}", std::process::id()));
    let data = root.join("data");
    let sc = root.join("spacecraft");
    fs::create_dir_all(&data).unwrap();
    fs::create_dir_all(&sc).unwrap();
    fs::write(sc.join("p.pth"), bytes(&[[1., 0., 0., 0.]])).unwrap();
    fs::write(sc.join("a.dat"), b"Orbit: P.PTH\n[ja]\nNAME: \x88\x90\r\n").unwrap();
    fs::write(sc.join("b.dat"), b"Orbit: ../../p.pth\nSize: unknown\n").unwrap();
    fs::write(sc.join("spacecraft.dat"), b"A.DAT\r\na.dat\r\n").unwrap();
    let d = Dataset::load(&data, Limits::default());
    let src = |key: &str| d.sources.iter().find(|s| s.key.ends_with(key)).unwrap();
    let SourceData::Definition(a) = src("/a.dat").data.as_ref().unwrap() else {
        panic!()
    };
    assert!(a.valid && a.trajectory.is_some());
    assert_eq!(a.field(b"NAME", b"ja"), Some(b"\x88\x90".as_slice()));
    let SourceData::Definition(b) = src("/b.dat").data.as_ref().unwrap() else {
        panic!()
    };
    assert!(!b.valid && b.trajectory.is_none());
    let r = src("/spacecraft.dat");
    let SourceData::Registration(entries) = r.data.as_ref().unwrap() else {
        panic!()
    };
    assert_eq!(entries.len(), 2);
    assert!(entries[0].definition.is_some());
    assert_eq!(entries[0].definition, entries[1].definition);
    assert_eq!(r.record_bytes(1).unwrap(), b"a.dat\r\n");
    fs::remove_dir_all(root).unwrap();
}
#[test]
#[ignore = "requires Mitaka Plus data"]
fn plus_trajectories_and_cpp_reference() {
    let data = PathBuf::from(std::env::var_os("MITAKA_DATA_ROOT").unwrap());
    let d = Dataset::load(&data, Limits::default());
    assert!(d.directory_errors.is_empty());
    assert_eq!(d.sources.len(), 21);
    let (mut definitions, mut paths, mut samples, mut duplicates) = (0, 0, 0, 0);
    for s in &d.sources {
        assert!(s.error.is_none());
        match s.data.as_ref().unwrap() {
            SourceData::Definition(v) => {
                definitions += 1;
                assert!(v.valid && v.trajectory.is_some());
            }
            SourceData::Pth(p) => {
                paths += 1;
                samples += p.samples().len();
                duplicates += p.duplicate_ordinals.len();
                for row in p.samples() {
                    assert_eq!(p.position(row.time).unwrap().sampling, Sampling::Exact);
                }
                if s.key.ends_with("Cassini.pth") {
                    let loc = p.position(2451545.123).unwrap();
                    assert_eq!((loc.first, loc.second), (678, 679));
                    for (v, r) in loc.position_au.iter().zip([
                        1.998355592748078,
                        1.3518074455205822,
                        -0.02811796282847885,
                    ]) {
                        assert!((v - r).abs() < 2e-14);
                    }
                }
            }
            _ => panic!("unexpected fixture source"),
        }
    }
    assert_eq!(
        (definitions, paths, samples, duplicates),
        (14, 7, 55405, 14)
    );
}
