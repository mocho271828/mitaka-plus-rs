//! Mitaka_plus f9fe624 grid_geometry.cpp。黄道は黄緯0°の大円として表示する。
use mitaka_core::astronomy::{
    equatorial_coordinates, equatorial_direction, j2000_ecliptic_to_equatorial,
};
use std::sync::OnceLock;

#[derive(Clone, Copy, PartialEq)]
pub enum System {
    Equatorial,
    Ecliptic,
    Galactic,
    Horizontal,
}
type Lines = Vec<Vec<[f64; 2]>>;
static EQUATORIAL: OnceLock<Lines> = OnceLock::new();
static ECLIPTIC: OnceLock<Lines> = OnceLock::new();
static GALACTIC: OnceLock<Lines> = OnceLock::new();
static HORIZONTAL: OnceLock<Lines> = OnceLock::new();
pub(super) type Vector = [f64; 3];
fn dot(a: Vector, b: Vector) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: Vector, b: Vector) -> Vector {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn galactic_basis() -> &'static [Vector; 3] {
    static BASIS: OnceLock<[Vector; 3]> = OnceLock::new();
    BASIS.get_or_init(|| {
        let c = equatorial_direction(
            (17. + 45. / 60. + 40.0383 / 3600.) * 15.,
            -(29. + 28.069 / 3600.),
        )
        .unwrap();
        let n =
            equatorial_direction((12. + 51. / 60. + 24. / 3600.) * 15., 27. + 8. / 60.).unwrap();
        [c, cross(n, c), n]
    })
}
pub(crate) fn galactic_to_j2000(v: Vector) -> Vector {
    let basis = galactic_basis();
    std::array::from_fn(|i| basis[0][i] * v[0] + basis[1][i] * v[1] + basis[2][i] * v[2])
}
pub(super) fn galactic_from_j2000(v: Vector) -> Vector {
    // GC と GN の公称位置は厳密に直交しないため、順変換の基底の逆行列を使う。
    static INVERSE: OnceLock<[Vector; 3]> = OnceLock::new();
    let axes = INVERSE.get_or_init(|| {
        let [c, e, n] = *galactic_basis();
        let x = cross(e, n);
        let det = dot(c, x);
        [x, cross(n, c), cross(c, e)].map(|axis| axis.map(|v| v / det))
    });
    axes.map(|axis| dot(axis, v))
}
fn to_equatorial(system: System, point: [f64; 2]) -> [f64; 2] {
    if matches!(system, System::Equatorial | System::Horizontal) {
        return point;
    }
    let v = equatorial_direction(point[0], point[1]).expect("grid angle");
    let v = if system == System::Ecliptic {
        j2000_ecliptic_to_equatorial(v).expect("unit vector")
    } else {
        galactic_to_j2000(v)
    };
    equatorial_coordinates(v).expect("nonzero grid direction")
}
pub fn lines(system: System) -> &'static Lines {
    let store = match system {
        System::Equatorial => &EQUATORIAL,
        System::Ecliptic => &ECLIPTIC,
        System::Galactic => &GALACTIC,
        System::Horizontal => &HORIZONTAL,
    };
    store.get_or_init(|| {
        let mut out = vec![];
        if system != System::Ecliptic {
            for lon in (0..360).step_by(if system == System::Horizontal { 45 } else { 30 }) {
                out.push(
                    (-90..=90)
                        .step_by(2)
                        .map(|lat| to_equatorial(system, [lon as f64, lat as f64]))
                        .collect(),
                );
            }
        }
        let limit = if system == System::Ecliptic { 0 } else { 75 };
        for lat in (-limit..=limit).step_by(15) {
            let mut line: Vec<_> = (0..360)
                .step_by(2)
                .map(|lon| to_equatorial(system, [lon as f64, lat as f64]))
                .collect();
            line.push(line[0]);
            out.push(line);
        }
        out
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn frame_landmarks() {
        assert_eq!(lines(System::Ecliptic).len(), 1);
        assert_eq!(lines(System::Horizontal).len(), 19);
        assert!((to_equatorial(System::Ecliptic, [90., 0.])[1] - 23.439291111).abs() < 1e-7);
        let center = to_equatorial(System::Galactic, [0., 0.]);
        assert!((center[0] - 266.41682625).abs() < 1e-7);
        assert!((center[1] + 29.00779694).abs() < 1e-7);
    }
}
