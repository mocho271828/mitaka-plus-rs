use mitaka_core::eclipse::{
    Sphere,
    display::{Options, Surface, display_cones},
};
fn ball(x: f64, r: f64) -> Sphere {
    Sphere {
        center_au: [x, 0., 0.],
        radius_au: r,
    }
}
#[test]
fn finite_fans_and_independent_surface_flags_match_native_definition() {
    let sun = ball(0., 2.);
    let occ = ball(10., 1.);
    let receiver = ball(14., 0.5);
    for mask in 0..16 {
        let o = Options {
            shadows: mask & 1 != 0,
            borders: mask & 2 != 0,
            umbra: mask & 4 != 0,
            penumbra: mask & 8 != 0,
        };
        let cones = display_cones(sun, occ, receiver, o).unwrap();
        assert_eq!(
            cones.len(),
            usize::from(o.umbra) + 2 * usize::from(o.penumbra)
        );
        if o.umbra {
            let c = cones[0];
            assert_eq!(c.apex, [20., 0., 0.]);
            assert_eq!(c.axis, [-1., 0., 0.]);
            assert!((c.near - 0.3 * 19.6).abs() < 1e-12);
            assert!((c.far - 19.6).abs() < 1e-12);
            assert!((c.slope - 3.84_f64.sqrt() / 19.6).abs() < 1e-12);
        }
        if o.penumbra {
            let c = cones[cones.len() - 1];
            assert!((c.slope - 3. / 91_f64.sqrt()).abs() < 1e-12);
            assert!((c.far - 40. / 3.).abs() < 1e-12);
        }
        let effect = Surface::bind(sun, receiver, &[occ], o, [0.3, 0., 0.]).unwrap();
        assert_eq!(effect.is_some(), o.shadows || o.borders);
        if let Some(effect) = effect {
            let lit = effect.effect([-1., 0., 0.], 0.).unwrap().multiplier;
            assert_eq!(lit[1..], if o.shadows { [0., 0.] } else { [1., 1.] });
            assert!((lit[0] - if o.shadows { 0.3 } else { 1. }).abs() < 1e-12);
            assert_eq!(effect.effect([1., 0., 0.], 0.).unwrap().multiplier, [1.; 3]);
            assert_eq!(
                effect.effect([-1., 0., 0.], 10.).unwrap().border_rgb,
                if o.borders { Some([1., 0.5, 0.]) } else { None }
            );
            assert!(effect.effect([2., 0., 0.], 0.).is_err());
        }
    }
}
#[test]
fn penumbra_and_umbra_have_independent_boundary_colors_and_lunar_transmission() {
    let binding = Surface::bind(
        ball(0., 2.),
        ball(14., 2.5),
        &[ball(10., 1.)],
        Options {
            shadows: true,
            borders: true,
            ..Default::default()
        },
        [0.3, 0., 0.],
    )
    .unwrap()
    .unwrap();
    let (mut umbra, mut penumbra, mut r, mut g) = (0, 0, 0., 0.);
    for i in 0..=1000 {
        let (s, c) = (f64::from(i) * std::f64::consts::FRAC_PI_2 / 1000.).sin_cos();
        let e = binding.effect([-c, s, 0.], 0.01).unwrap();
        match e.border_rgb {
            Some([1., 0.5, 0.]) => umbra += 1,
            Some([1., 0., 0.]) => penumbra += 1,
            None => {}
            v => panic!("unexpected border {v:?}"),
        }
        r += e.multiplier[0];
        g += e.multiplier[1];
    }
    // 原版 EclipseSurfaceBinding::effect を同じ1001法線で評価した結果。
    assert_eq!((umbra, penumbra), (5, 11));
    assert!((r - 778.198_834_601_840_4).abs() < 1e-9);
    assert!((g - 682.712_620_859_772_4).abs() < 1e-9);
}
