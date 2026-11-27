use super::*;
use crate::layers::LayerOptions;

fn references() -> [Raster; 2] {
    let frame = ObservationFrame::new(35.68, 139.76, 2459200.5).unwrap();
    [
        Raster::new(
            View::Sky,
            Camera {
                longitude: 266.41682625,
                latitude: -29.0077969444,
                zoom: 4.,
                roll: 0.,
            },
            &frame,
        )
        .unwrap(),
        Raster::new(
            View::Horizon,
            Camera {
                longitude: 279.,
                latitude: -15.,
                zoom: 180. / 70.,
                roll: 33.,
            },
            &frame,
        )
        .unwrap(),
    ]
}
fn colors(texture: &RgbaImage, raster: Raster) -> [[u8; 3]; 9] {
    let image = raster.render(texture, [9, 5]);
    [0, 4, 8, 18, 22, 26, 36, 40, 44].map(|i| {
        let [r, g, b, a] = image.pixels[i].to_array();
        assert_eq!(a, 0, "background must composite additively");
        [r, g, b]
    })
}
#[test]
fn cpp_raster_values_match_across_sky_and_rolled_horizon() {
    let img = RgbaImage::from_fn(64, 32, |x, y| {
        image::Rgba([(4 * x) as u8, (8 * y) as u8, 0, 255])
    });
    let [sky, horizon] = references();
    // f9fe624 render_sky_background / render_horizon_background の出力。
    assert_eq!(
        colors(&img, sky),
        [
            [102, 161, 0],
            [115, 111, 0],
            [135, 67, 0],
            [116, 170, 0],
            [126, 124, 0],
            [144, 88, 0],
            [131, 174, 0],
            [137, 137, 0],
            [151, 110, 0]
        ]
    );
    assert_eq!(
        colors(&img, horizon),
        [
            [177, 108, 0],
            [214, 71, 0],
            [15, 45, 0],
            [184, 145, 0],
            [225, 106, 0],
            [15, 85, 0],
            [194, 181, 0],
            [233, 142, 0],
            [15, 125, 0]
        ]
    );
}
#[test]
fn seam_poles_transparency_and_lua_mode() {
    let image = RgbaImage::from_fn(2, 2, |x, y| {
        image::Rgba(if y == 1 {
            [0, 0, 255, 255]
        } else if x == 0 {
            [255, 0, 0, 255]
        } else {
            [0, 255, 0, 0]
        })
    });
    assert_eq!(sample(&image, [0., 0.]), [255, 0, 0, 128]);
    assert_eq!(sample(&image, [1., 0.]), [255, 0, 0, 128]);
    assert_eq!(sample(&image, [0.75, 0.]), [0; 4]);
    assert_eq!(sample(&image, [0.5, 1.]), [0, 0, 255, 255]);
    assert_eq!(uv([0., 0., 1.]), [0.5, 0.]);
    let mut opt = LayerOptions::default();
    for mode in Mode::ALL {
        assert!(opt
            .action(21, &serde_json::json!({"text":mode.name()}))
            .unwrap());
        assert_eq!(opt.milky_way, mode);
        assert_eq!(opt.snapshot(serde_json::json!({}))["milkyWay"], mode.name());
    }
    assert!(opt
        .action(21, &serde_json::json!({"text":"unknown"}))
        .is_err());
    assert_eq!(opt.milky_way, Mode::Infrared);
}
#[test]
fn missing_images_report_independently_and_infrared_uses_only_first_present_name() {
    let root = tempfile::tempdir().unwrap();
    let tex = root.path().join("textures");
    fs::create_dir(&tex).unwrap();
    let img = image::RgbImage::from_pixel(4, 2, image::Rgb([55, 30, 10]));
    img.save(tex.join("milkyway_IR.jpg")).unwrap();
    let mut warnings = vec![];
    let loaded = Backgrounds::load(&root.path().join("data"), &mut warnings);
    assert_eq!(warnings.len(), 2);
    assert!(loaded.get(Mode::Infrared).is_some());
    // 大小文字を区別しないボリュームでも同じ期待値になるよう、別名を残すのは区別する場合のみ。
    fs::write(tex.join("MilkyWay_IR.jpg"), b"broken").unwrap();
    warnings.clear();
    let loaded = Backgrounds::load(&root.path().join("data"), &mut warnings);
    assert_eq!(warnings.len(), 3);
    assert!(loaded.get(Mode::Infrared).is_none());
}
#[test]
#[ignore = "requires original Plus 1.5.4 textures in MITAKA_DATA_ROOT/../textures"]
fn distributed_images_match_cpp_in_both_views() {
    let data = std::env::var_os("MITAKA_DATA_ROOT").unwrap();
    let mut warnings = vec![];
    let images = Backgrounds::load(Path::new(&data), &mut warnings);
    assert!(warnings.is_empty(), "{warnings:?}");
    let [sky, horizon] = references();
    // C++ の ImageIO と Rust JPEG 復号器は端数処理が異なる。
    let references = [
        [
            [0, 0, 0],
            [5, 5, 5],
            [0, 0, 0],
            [0, 0, 0],
            [24, 23, 23],
            [0, 0, 0],
            [0, 0, 0],
            [17, 16, 24],
            [7, 6, 11],
        ],
        [
            [0, 0, 0],
            [0, 0, 0],
            [0, 0, 0],
            [2, 2, 2],
            [0, 0, 0],
            [0, 0, 0],
            [0, 0, 0],
            [0, 0, 0],
            [1, 1, 1],
        ],
        [
            [0, 0, 12],
            [0, 1, 1],
            [0, 0, 8],
            [1, 0, 5],
            [37, 27, 34],
            [0, 0, 9],
            [1, 1, 3],
            [24, 24, 56],
            [11, 9, 33],
        ],
        [
            [1, 1, 2],
            [0, 0, 0],
            [0, 0, 12],
            [0, 0, 3],
            [1, 1, 3],
            [0, 0, 13],
            [0, 0, 0],
            [5, 0, 0],
            [0, 0, 0],
        ],
        [
            [59, 75, 94],
            [127, 137, 139],
            [40, 40, 57],
            [44, 47, 81],
            [216, 182, 128],
            [57, 66, 95],
            [43, 49, 77],
            [119, 138, 155],
            [105, 125, 144],
        ],
        [
            [81, 102, 132],
            [26, 30, 52],
            [20, 18, 43],
            [64, 84, 112],
            [62, 76, 107],
            [41, 41, 67],
            [31, 30, 54],
            [59, 70, 101],
            [97, 109, 125],
        ],
    ];
    for (i, mode) in [Mode::Normal, Mode::Bright, Mode::Infrared]
        .into_iter()
        .enumerate()
    {
        let texture = images.get(mode).unwrap();
        assert_eq!(texture.dimensions(), (1024, 512));
        for (j, view) in [sky, horizon].into_iter().enumerate() {
            let actual = colors(texture, view);
            let expected = references[i * 2 + j];
            let max = actual
                .iter()
                .flatten()
                .zip(expected.iter().flatten())
                .map(|(a, b)| a.abs_diff(*b))
                .max()
                .unwrap();
            assert!(
                max <= 3,
                "{mode:?} view {j} max {max}: {actual:?} vs {expected:?}"
            );
        }
    }
}
