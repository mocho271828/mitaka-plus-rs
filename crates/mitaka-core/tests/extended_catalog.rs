use mitaka_core::{catalog::CatalogLimits, extended_catalog::*};
use std::path::PathBuf;
#[test]
fn validates_catalog_specific_numbers_and_coordinates() {
    let l = CatalogLimits::default();
    let mut ngc = 1i16.to_le_bytes().to_vec();
    ngc.push(1);
    for v in [30f32, -90.] {
        ngc.extend(v.to_le_bytes());
    }
    ngc.push(11);
    for v in [2f32, -1.] {
        ngc.extend(v.to_le_bytes());
    }
    ngc.push(0);
    assert_eq!(parse_ngc(&ngc, l).unwrap()[0].kind, 11);
    for i in 1..ngc.len() {
        assert!(parse_ngc(&ngc[..i], l).is_err());
    }
    assert!(parse_ngc(&ngc.repeat(2), l).is_err());
    ngc[2] = 2;
    assert!(parse_ngc(&ngc, l).is_err());
    let mut sdss = vec![];
    for v in [120f32, 30., 4e6, -1.1] {
        sdss.extend(v.to_le_bytes());
    }
    assert_eq!(
        parse_sdss(&sdss, SdssKind::Galaxy, l).unwrap()[0].distance_pc,
        4e6
    );
    sdss[0..4].copy_from_slice(&360f32.to_le_bytes());
    assert!(parse_sdss(&sdss, SdssKind::Galaxy, l).is_err());
}
#[test]
fn hip_text_preserves_legacy_names_and_repeated_hosts() {
    let l = CatalogLimits::default();
    let raw = b"% comment\r\n   123*Star  \x83\x5c\r\n";
    let row = parse_star_names(raw, l).unwrap().remove(0);
    assert_eq!(row.hip, 123);
    assert_eq!(row.source_row, 2);
    assert_eq!(row.localized_name_bytes, [0x83, 0x5c]);
    assert!(row.featured);
    assert!(parse_star_names(&raw.repeat(2), l).is_err());
    assert_eq!(parse_exoplanet_hosts(b"# ids\n1\r\n1", l).unwrap(), [1, 1]);
    for bad in [b"+1".as_slice(), b"3 4", b"0", b"1\0"] {
        assert!(parse_exoplanet_hosts(bad, l).is_err());
    }
}
#[test]
#[ignore = "requires an explicitly supplied Plus 1.5.4 data directory"]
fn extended_real_data() {
    let dir = PathBuf::from(std::env::var_os("MITAKA_DATA_ROOT").unwrap());
    assert!(!load_ngc(&dir.join("ngc.dat")).unwrap().is_empty());
    assert_eq!(
        load_sdss(&dir.join("SDSS_GAL.dat"), SdssKind::Galaxy)
            .unwrap()
            .len(),
        826360
    );
    assert_eq!(
        load_sdss(&dir.join("SDSS_QSO.dat"), SdssKind::Quasar)
            .unwrap()
            .len(),
        104848
    );
    assert!(
        !load_star_names(&dir.join("star_names_jp.dat"))
            .unwrap()
            .is_empty()
    );
    assert!(
        !load_exoplanet_hosts(&dir.join("exoplanets_HIP_list.dat"))
            .unwrap()
            .is_empty()
    );
}
