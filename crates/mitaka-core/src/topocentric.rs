//! Mitaka_plus f9fe624 horizon_objects.cpp の地球上の移動天体用の視差補正。
//! 地球基準半径とWGS84扁平率、標高0 km。入力時刻に対する幾何学的位置を扱う。
use crate::{
    astronomy::{
        ObservationFrame, Vector3, equatorial_coordinates, equatorial_direction,
        j2000_ecliptic_to_equatorial,
    },
    ephemeris::Cf96Ephemeris,
    lunar::{earth_center, lunar_position},
};
use std::io;
const EQUATORIAL_RADIUS_AU: f64 = 6378.1366 / 149_597_870.7;
const FLATTENING: f64 = 1. / 298.257223563;
#[derive(Clone, Copy, Debug)]
pub struct Observer {
    frame: ObservationFrame,
    center: Vector3,
    offset: Vector3,
}
#[derive(Clone, Copy, Debug)]
pub struct Apparent {
    pub equatorial_deg: [f64; 2],
    pub horizontal_deg: [f64; 2],
    pub distance_au: f64,
}
impl Observer {
    pub fn new(
        latitude: f64,
        longitude: f64,
        jd: f64,
        earth_center_equatorial_au: Vector3,
    ) -> io::Result<Self> {
        if !earth_center_equatorial_au.iter().all(|v| v.is_finite()) {
            return Err(io::Error::other("non-finite Earth position"));
        }
        let frame = ObservationFrame::new(latitude, longitude, jd)?;
        let axes = |alt| -> io::Result<Vector3> {
            let [ra, dec] = frame.to_equatorial(0., alt)?;
            equatorial_direction(ra, dec)
        };
        let up = axes(90.)?;
        let north = axes(0.)?;
        let (s, c) = latitude.to_radians().sin_cos();
        let e2 = FLATTENING * (2. - FLATTENING);
        let n = EQUATORIAL_RADIUS_AU / (1. - e2 * s * s).sqrt();
        let u = n * (1. - e2 * s * s);
        let v = -n * e2 * s * c;
        let offset = std::array::from_fn(|i| u * up[i] + v * north[i]);
        Ok(Self {
            frame,
            center: earth_center_equatorial_au,
            offset,
        })
    }
    pub fn from_ephemeris(
        eph: &Cf96Ephemeris,
        jd: f64,
        latitude: f64,
        longitude: f64,
    ) -> io::Result<Self> {
        let center = earth_center(eph.state(2, jd)?.position_au, lunar_position(jd)?);
        Self::new(latitude, longitude, jd, center)
    }
    pub fn offset_equatorial_au(self) -> Vector3 {
        self.offset
    }
    pub fn observe(self, heliocentric_ecliptic_au: Vector3) -> io::Result<Apparent> {
        let eq = j2000_ecliptic_to_equatorial(heliocentric_ecliptic_au)?;
        // 遠距離の太陽中心座標を先に相殺し、数十µAUの地点位置を保持する。
        let v: Vector3 = std::array::from_fn(|i| (eq[i] - self.center[i]) - self.offset[i]);
        let d = v[0].hypot(v[1]).hypot(v[2]);
        if !d.is_finite() || d <= 0. {
            return Err(io::Error::other("coincident or non-finite horizon object"));
        }
        let equatorial_deg = equatorial_coordinates(v)?;
        let horizontal_deg = self
            .frame
            .to_horizontal(equatorial_deg[0], equatorial_deg[1])?;
        Ok(Apparent {
            equatorial_deg,
            horizontal_deg,
            distance_au: d,
        })
    }
}
