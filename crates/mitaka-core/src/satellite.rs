//! Satellite rotation and parent-relative elliptic orbit evaluated at one legacy
//! numerical JD. Based on Mitaka_plus f9fe624 satellite_rotation/position.cpp.
use crate::{
    astronomy::{J2000, Vector3, centered, rotate_x, rotate_z},
    orbit::{OrbitalElements, orbital_state},
};
use std::io;
fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}
#[derive(Clone, Copy, Debug)]
pub struct ParentPole {
    pub jd: f64,
    pub ra_deg: f64,
    pub dec_deg: f64,
}
#[derive(Clone, Copy, Debug)]
pub struct SatelliteElements {
    pub epoch_jd: f64,
    pub semi_major_axis_km: f64,
    pub eccentricity: f64,
    pub mean_anomaly_deg: f64,
    pub mean_motion_deg_per_day: f64,
    pub argument_of_periapsis_deg: f64,
    pub inclination_deg: f64,
    pub raw_node_deg: f64,
    pub periapsis_period_years: f64,
    pub node_period_years: f64,
    pub laplace_pole_deg: Option<[f64; 2]>,
}
#[derive(Clone, Debug)]
pub struct RotationInput {
    pub name_bytes: Vec<u8>,
    pub reference_plane: u8,
    pub elements: Option<SatelliteElements>,
    pub correct_mars_node: bool,
    pub charon_index: Option<usize>,
    pub parent_pole: Option<ParentPole>,
}
pub type Axes = [Vector3; 3];
const IDENTITY: Axes = [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]];
#[derive(Clone, Copy, Debug)]
pub struct TimedRotation {
    pub jd: f64,
    pub axes: Axes,
}
fn apply(axes: Axes, p: Vector3) -> Vector3 {
    std::array::from_fn(|i| axes[0][i] * p[0] + axes[1][i] * p[1] + axes[2][i] * p[2])
}
fn rate(period: f64) -> f64 {
    if period == 0. {
        0.
    } else {
        360. / period / 365.25
    }
}
pub fn legacy_parent_pole(registration_index: usize, jd: f64) -> io::Result<ParentPole> {
    let rows = [
        [281.01, -0.033, 61.45, -0.005],
        [272.76, 0., 67.16, 0.],
        [0., -0.641, 90., -0.557],
        [317.68143, -0.1061, 52.8865, -0.0609],
        [268.05, -0.009, 64.49, 0.003],
        [40.589, -0.036, 83.537, -0.004],
        [257.311, 0., -15.175, 0.],
        [299.36, 0., 43.46, 0.],
        [313.02, 0., 9.09, 0.],
    ];
    let &[a, b, c, d] = rows
        .get(registration_index)
        .ok_or_else(|| invalid("unverified legacy planet registration index"))?;
    if !jd.is_finite() {
        return Err(invalid("non-finite satellite JD"));
    }
    let centuries = (jd - J2000) / 36525.;
    let (mut ra, mut dec) = (a + b * centuries, c + d * centuries);
    if registration_index == 7 {
        let (s, c) = centered(357.85 + 52.316 * centuries, 360.)
            .to_radians()
            .sin_cos();
        ra += 0.7 * s;
        dec -= 0.51 * c;
    }
    Ok(ParentPole {
        jd,
        ra_deg: ra,
        dec_deg: dec,
    })
}
fn local_rotation(input: &RotationInput, jd: f64) -> io::Result<Axes> {
    if input.name_bytes == b"Moon" {
        return Ok(IDENTITY);
    }
    let plane = input.reference_plane;
    if !b"LEICQ".contains(&plane) {
        return Err(invalid("unknown satellite reference plane"));
    }
    let e = input
        .elements
        .ok_or_else(|| invalid("missing rotation elements"))?;
    if ![
        e.epoch_jd,
        e.argument_of_periapsis_deg,
        e.inclination_deg,
        e.raw_node_deg,
        e.periapsis_period_years,
        e.node_period_years,
    ]
    .iter()
    .all(|v| v.is_finite())
    {
        return Err(invalid("non-finite satellite rotation elements"));
    }
    let elapsed = jd - e.epoch_jd;
    let w = e.argument_of_periapsis_deg + elapsed * rate(e.periapsis_period_years);
    let node = e.raw_node_deg
        + if input.correct_mars_node { 43. } else { 0. }
        + elapsed * rate(e.node_period_years);
    if !w.is_finite() || !node.is_finite() {
        return Err(invalid("satellite precession overflow"));
    }
    let mut axes = IDENTITY.map(|v| rotate_z(rotate_x(rotate_z(v, w), e.inclination_deg), node));
    if plane == b'L' {
        let [ra, dec] = e
            .laplace_pole_deg
            .ok_or_else(|| invalid("missing Laplace pole"))?;
        if !ra.is_finite() || !(-90.0..=90.).contains(&dec) {
            return Err(invalid("invalid Laplace pole"));
        }
        axes = axes.map(|v| rotate_z(rotate_x(v, 90. - dec), 90. + centered(ra, 360.)));
    }
    if matches!(plane, b'I' | b'L') {
        axes = axes.map(|v| rotate_x(v, 23.43870583));
    }
    if plane == b'Q' {
        let p = input
            .parent_pole
            .ok_or_else(|| invalid("missing parent pole"))?;
        if p.jd != jd || ![p.ra_deg, p.dec_deg].iter().all(|v| v.is_finite()) {
            return Err(invalid("invalid or mismatched parent pole"));
        }
        axes = axes.map(|v| {
            rotate_x(
                rotate_z(rotate_x(v, 90. - p.dec_deg), 90. + centered(p.ra_deg, 360.)),
                -23.439291111111114,
            )
        });
    }
    Ok(axes)
}
pub fn satellite_rotation(
    records: &[RotationInput],
    mut index: usize,
    jd: f64,
) -> io::Result<TimedRotation> {
    if !jd.is_finite() {
        return Err(invalid("non-finite satellite JD"));
    }
    let mut visited = vec![false; records.len()];
    let mut pending: Vec<Axes> = Vec::new();
    loop {
        let r = records
            .get(index)
            .ok_or_else(|| invalid("satellite index out of bounds"))?;
        if visited[index] {
            return Err(invalid("cyclic satellite rotation dependency"));
        }
        visited[index] = true;
        let mut axes =
            local_rotation(r, jd).map_err(|e| invalid(&format!("satellite {index}: {e}")))?;
        if r.reference_plane != b'C' || r.name_bytes == b"Moon" {
            for child in pending.into_iter().rev() {
                axes = child.map(|v| apply(axes, v));
            }
            return Ok(TimedRotation { jd, axes });
        }
        pending.push(axes);
        index = r
            .charon_index
            .ok_or_else(|| invalid("unresolved Charon dependency"))?;
    }
}
fn dot(a: Vector3, b: Vector3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn orthonormal([x, y, z]: Axes) -> bool {
    let cross = [
        x[1] * y[2] - x[2] * y[1],
        x[2] * y[0] - x[0] * y[2],
        x[0] * y[1] - x[1] * y[0],
    ];
    [
        dot(x, x) - 1.,
        dot(y, y) - 1.,
        dot(z, z) - 1.,
        dot(x, y),
        dot(x, z),
        dot(y, z),
        dot(cross, z) - 1.,
    ]
    .iter()
    .all(|v| v.abs() <= 1e-10)
}
/// Position relative to the uncorrected parent, in km, in the rotation's frame.
pub fn relative_position_km(
    e: SatelliteElements,
    rotation: TimedRotation,
    jd: f64,
) -> io::Result<Vector3> {
    if jd != rotation.jd || !jd.is_finite() {
        return Err(invalid("invalid or mismatched satellite rotation JD"));
    }
    if !e.semi_major_axis_km.is_finite()
        || e.semi_major_axis_km <= 0.
        || !(0.0..1.).contains(&e.eccentricity)
        || !e.periapsis_period_years.is_finite()
        || !e.node_period_years.is_finite()
    {
        return Err(invalid("invalid elliptic satellite orbit"));
    }
    if !orthonormal(rotation.axes) {
        return Err(invalid("invalid right-handed satellite rotation"));
    }
    let n = e.mean_motion_deg_per_day - rate(e.periapsis_period_years) - rate(e.node_period_years);
    let shape = orbital_state(
        OrbitalElements {
            epoch_jd: e.epoch_jd,
            semi_major_axis_au: 1.,
            eccentricity: e.eccentricity,
            mean_anomaly_deg: e.mean_anomaly_deg,
            mean_motion_deg_per_day: n,
            ascending_node_deg: 0.,
            inclination_deg: 0.,
            argument_of_periapsis_deg: 0.,
        },
        jd,
    )?;
    let p = apply(rotation.axes, shape.position_au).map(|v| v * e.semi_major_axis_km);
    if p.iter().all(|v| v.is_finite()) {
        Ok(p)
    } else {
        Err(invalid("satellite position overflow"))
    }
}
