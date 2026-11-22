use mitaka_core::catalog::*;

#[test]
fn hip_preserves_signed_fields_duplicates_and_source_bytes() {
    let mut raw = 0x01020304i32.to_le_bytes().to_vec();
    for v in [1.0f32, -2.0, -0.5, 0.1, 125.0, -256.0, 4.5, 0.75] {
        raw.extend(v.to_le_bytes());
    }
    raw.extend([b'G', 2, 1, 254, 255, 255, 3, 42, b'A']);
    let limits = CatalogLimits::default();
    for len in 1..45 {
        assert!(parse_hipparcos(&raw[..len], limits).is_err());
    }
    let rows = parse_hipparcos(&raw.repeat(2), limits).unwrap();
    assert_eq!(rows[0].raw.as_slice(), raw);
    assert_eq!(rows[1].source_row, 2);
    assert_eq!(rows[0].hip, 0x01020304);
    assert_eq!(rows[0].parallax_mas, -0.5);
    assert_eq!(rows[0].flamsteed_number, -2);
    assert!(rows[0].is_double);
    assert_eq!(rows[0].bayer_id, -1);
    assert_eq!(rows[0].proper_motion_dec_mas_per_year, -256.);
    assert!(parse_hipparcos(&[], limits).unwrap().is_empty());
    assert!(
        parse_hipparcos(
            &raw,
            CatalogLimits {
                max_file_bytes: 44,
                ..limits
            }
        )
        .is_err()
    );
    assert!(
        parse_hipparcos(
            &raw,
            CatalogLimits {
                max_records: 0,
                ..limits
            }
        )
        .is_err()
    );
    raw[38] = 2;
    assert!(parse_hipparcos(&raw, limits).is_err());
    raw[38] = 0;
    raw[4..8].copy_from_slice(&f32::NAN.to_le_bytes());
    assert!(parse_hipparcos(&raw, limits).is_err());
}

#[test]
fn constellation_validates_every_prefix_and_preserves_name_encoding() {
    let limits = CatalogLimits::default();
    let mut row = vec![];
    for name in [
        b"And".as_slice(),
        b"And",
        b"Andromeda",
        b"Andromedae",
        &[0x83, 0x41],
        &[0x90, 0xaf],
    ] {
        row.push(name.len() as u8);
        row.extend(name);
    }
    row.extend(15f32.to_le_bytes());
    row.extend((-40f32).to_le_bytes());
    row.extend([1, 2]);
    row.extend(0x01020304i32.to_le_bytes());
    row.extend(936850i32.to_le_bytes());
    let boundary_offset = row.len();
    row.extend(1i16.to_le_bytes());
    row.extend(24f32.to_le_bytes());
    row.extend((-30f32).to_le_bytes());
    for len in 1..row.len() {
        assert!(parse_constellations(&row[..len], limits).is_err());
    }
    let rows = parse_constellations(&row.repeat(2), limits).unwrap();
    assert_eq!(rows[0].raw, row);
    assert_eq!(rows[1].source_row, 2);
    assert_eq!(rows[0].japanese_kanji_bytes, [0x90, 0xaf]);
    assert_eq!(rows[0].hip_lines, [vec![0x01020304, 936850]]);
    assert_eq!(rows[0].boundary, [[24., -30.]]);
    row[boundary_offset..boundary_offset + 2].copy_from_slice(&(-1i16).to_le_bytes());
    assert!(parse_constellations(&row, limits).is_err());
    assert!(parse_constellations(&[], limits).unwrap().is_empty());
}
