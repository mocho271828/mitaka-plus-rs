use mitaka_core::{
    catalog::CatalogLimits,
    small_body::{Dataset, Encoding, Orbit, Perihelion},
};
use std::{fs, path::PathBuf};

fn near(actual: [f64; 3], expected: [f64; 3], tolerance: f64) {
    for (a, e) in actual.into_iter().zip(expected) {
        assert!((a - e).abs() < tolerance, "{a} != {e}");
    }
}
#[test]
fn parabolic_state_keeps_radius_and_tangential_speed_at_perihelion() {
    let p = Perihelion {
        distance_au: 2.,
        eccentricity: 1.,
        jd: 2451545.,
        ascending_node_deg: 0.,
        inclination_deg: 0.,
        argument_deg: 90.,
    };
    let s = p.state(p.jd).unwrap();
    near(s.position_au, [0., 2., 0.], 1e-14);
    near(s.velocity_au_per_day, [-0.01720209895, 0., 0.], 1e-14);
    assert!(p.state(f64::NAN).is_err());
}
#[test]
fn preserves_each_source_and_explicit_mean_motion() {
    let root = std::env::temp_dir().join(format!("mitaka-rs-small-body-{}", std::process::id()));
    fs::create_dir_all(root.join("orbits/asteroids")).unwrap();
    let bytes=b"Name: A\r\nDisplayName: \x8d\xc5\r\nType: Asteroid\nNo: 1\nEpoch: 2451545\na: 1\nn: 90\ni: 0\nnode: 0\nw: 0\ne: 0\nM0: 0\n";
    fs::write(root.join("orbits/asteroids/a.dat"), bytes).unwrap();
    fs::write(
        root.join("orbits/asteroids/b.dat"),
        [bytes.as_slice(), b"n: 3\n"].concat(),
    )
    .unwrap();
    fs::write(root.join("small_objects_name.dat"), b"1 Alpha\n1 Beta\r\n").unwrap();
    fs::write(root.join("mp.dat"), [0; 35]).unwrap();
    let d = Dataset::load(&root, CatalogLimits::default());
    let s = d
        .sources
        .iter()
        .find(|s| s.key.ends_with("/a.dat"))
        .unwrap();
    assert!(s.error.is_none());
    assert_eq!(s.record_bytes(0), Some(bytes.as_slice()));
    assert!(s.sha256.is_some());
    assert_eq!(s.records[0].display_name_bytes, b"\x8d\xc5");
    assert_eq!(
        s.records[0].aliases,
        [b"Alpha".as_slice(), b"Beta".as_slice()]
    );
    near(
        s.records[0].position(2451546.).unwrap(),
        [0., 1., 0.],
        1e-14,
    );
    for s in d
        .sources
        .iter()
        .filter(|s| s.key == "mp.dat" || s.key.ends_with("/b.dat"))
    {
        assert!(s.error.is_some() && s.records.is_empty() && s.sha256.is_some());
    }
    fs::remove_dir_all(root).unwrap();
}
#[test]
#[ignore = "requires Mitaka Plus data"]
fn plus_data_and_cpp_reference_vectors() {
    let root = PathBuf::from(std::env::var_os("MITAKA_DATA_ROOT").unwrap());
    let data = Dataset::load(&root, CatalogLimits::default());
    assert!(data.directory_errors.is_empty());
    assert_eq!(data.sources.len(), 67);
    assert_eq!(
        data.sources.iter().map(|s| s.records.len()).sum::<usize>(),
        19475
    );
    let mut counts = [0; 3];
    for s in &data.sources {
        assert!(s.error.is_none(), "{} {:?}", s.key, s.error);
        for (idx, r) in s.records.iter().enumerate() {
            assert!(s.record_bytes(idx).is_some_and(|b| !b.is_empty()));
            if r.encoding == Encoding::Packed37 {
                assert!(r.raw_flag.is_some() && r.position(2451545.).is_err());
            }
            if let Orbit::Perihelion(p) = r.orbit {
                counts[if p.eccentricity < 1. {
                    0
                } else if p.eccentricity == 1. {
                    1
                } else {
                    2
                }] += 1;
                for jd in [2415020.5, 2451545., 2488069.5] {
                    assert!(r.position(jd).is_ok());
                }
            }
        }
    }
    assert_eq!(counts, [859, 1298, 231]);
    for (key, expected) in [
        (
            "mp.dat",
            [-2.379802628910517, 0.7974688124709388, 0.4631255129762282],
        ),
        (
            "TNO.dat",
            [-29.26941562320587, -30.517400470782807, 0.21810544829361717],
        ),
        (
            "comets.dat",
            [-17.3842507930939, 16.962292970752785, -7.578329000115467],
        ),
        (
            "orbits/asteroids/ceres.dat",
            [-2.3798031758028206, 0.7974689180195999, 0.4631256277965633],
        ),
    ] {
        let s = data.sources.iter().find(|s| s.key == key).unwrap();
        near(s.records[0].position(2451545.).unwrap(), expected, 3e-12);
    }
}
