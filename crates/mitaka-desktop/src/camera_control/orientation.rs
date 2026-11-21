//! Mitaka Plusの姿勢は Y(heading) X(pitch) Z(-roll) の順で合成する。
use crate::{projection::Camera, solar::SolarCamera};
use glam::{DMat3, DQuat, DVec3, EulerRot};
use std::io;
pub(super) fn unit(v: [f64; 4]) -> Result<DQuat, String> {
    let size = v.iter().map(|x| x.abs()).fold(0., f64::max);
    if size == 0. || !v.iter().all(|x| x.is_finite()) {
        return Err("姿勢には有限な非零四元数が必要です".into());
    }
    Ok(DQuat::from_array(v.map(|n| n / size)).normalize())
}
pub(super) fn from_euler(v: [f64; 3]) -> DQuat {
    let [h, p, r] = v.map(|x| (x % 720.).to_radians());
    DQuat::from_euler(EulerRot::YXZ, h, p, -r)
}
pub(super) fn euler(q: DQuat) -> [f64; 3] {
    let m = DMat3::from_quat(q);
    let r02 = m.z_axis.x;
    let r22 = m.z_axis.z;
    let h = r02.hypot(r22);
    let p = (-m.z_axis.y).atan2(h);
    let (head, roll) = if h <= 1e-12 {
        (0., m.y_axis.x.atan2(m.x_axis.x))
    } else {
        (r02.atan2(r22), -m.x_axis.y.atan2(m.y_axis.y))
    };
    [head, p, roll].map(f64::to_degrees)
}
fn matrix(b: [[f64; 3]; 3]) -> DMat3 {
    DMat3::from_cols(b[0].into(), b[1].into(), b[2].into())
}
pub(super) fn solar(c: SolarCamera) -> DQuat {
    DQuat::from_mat3(&matrix(c.basis())).normalize()
}
pub(super) fn apply_solar(c: &mut SolarCamera, q: DQuat) {
    let m = DMat3::from_quat(q);
    let outward = m.z_axis;
    let (x, y, z) = (outward.x, outward.y, outward.z);
    if x.hypot(y) > 1e-12 {
        c.yaw = y.atan2(x).to_degrees();
    }
    c.pitch = z.atan2(x.hypot(y)).to_degrees();
    c.roll = 0.;
    c.frame = None;
    let base = matrix(c.basis());
    c.roll = m
        .x_axis
        .dot(base.y_axis)
        .atan2(m.x_axis.dot(base.x_axis))
        .to_degrees();
    c.frame = Some([
        m.x_axis.to_array(),
        m.y_axis.to_array(),
        m.z_axis.to_array(),
    ]);
    c.frame_is_orbit = true;
}
pub(super) fn site(lat: f64, lon: f64, jd: f64) -> io::Result<DMat3> {
    body_site(3, lat, lon, jd)
}
pub(super) fn body_site(body: usize, lat: f64, lon: f64, jd: f64) -> io::Result<DMat3> {
    let rot = matrix(if body == 10 {
        mitaka_core::lunar::lunar_orientation(jd)?
    } else {
        mitaka_core::rotation::planet_orientation(
            body.checked_sub(1)
                .ok_or_else(|| io::Error::other("太陽は地表姿勢に非対応です"))?,
            jd,
        )?
    });
    let (s, c) = lat.to_radians().sin_cos();
    let (t, d) = lon.to_radians().sin_cos();
    Ok(rot
        * DMat3::from_cols(
            DVec3::new(-t, d, 0.),
            DVec3::new(c * d, c * t, s),
            DVec3::new(s * d, s * t, -c),
        ))
}
fn surface(c: Camera, local: DMat3) -> DMat3 {
    let (sh, ch) = c.longitude.to_radians().sin_cos();
    let (se, ce) = c.latitude.to_radians().sin_cos();
    let east = local.x_axis;
    let up = local.y_axis;
    let north = -local.z_axis;
    let flat = north * ch + east * sh;
    let forward = flat * ce + up * se;
    let right = east * ch - north * sh;
    let right_up = up * ce - flat * se;
    let (sr, cr) = c.roll.to_radians().sin_cos();
    DMat3::from_cols(
        right * cr + right_up * sr,
        right_up * cr - right * sr,
        -forward,
    )
}
pub(super) fn horizon(c: Camera, local: DMat3) -> DQuat {
    DQuat::from_mat3(&surface(c, local)).normalize()
}
pub(super) fn apply_horizon(c: &mut Camera, local: DMat3, q: DQuat) {
    let m = DMat3::from_quat(q);
    let forward = -m.z_axis;
    let north = forward.dot(-local.z_axis);
    let east = forward.dot(local.x_axis);
    if north.hypot(east) > 1e-12 {
        c.longitude = east.atan2(north).to_degrees().rem_euclid(360.);
    }
    c.latitude = forward.dot(local.y_axis).clamp(-1., 1.).asin().to_degrees();
    c.roll = 0.;
    let base = surface(*c, local);
    c.roll = m
        .x_axis
        .dot(base.y_axis)
        .atan2(m.x_axis.dot(base.x_axis))
        .to_degrees();
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn legacy_euler_and_site_frames_round_trip() {
        for v in [[20., -32., 15.], [90., 90., 120.], [-150., 89., 27.]] {
            let q = from_euler(v);
            assert!(q.dot(from_euler(euler(q))).abs() > 1. - 1e-13);
            let mut c = SolarCamera::default();
            apply_solar(&mut c, q);
            assert!(q.dot(solar(c)).abs() > 1. - 1e-13);
            let f = site(35.68, 139.76, 2451545.).unwrap();
            let mut g = Camera::default();
            apply_horizon(&mut g, f, q);
            assert!(q.dot(horizon(g, f)).abs() > 1. - 1e-13);
        }
        assert!(unit([f64::MAX; 4]).is_ok());
        assert!(unit([0.; 4]).is_err());
    }
}
