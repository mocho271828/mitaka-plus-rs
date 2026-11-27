//! 円筒投影。現行版の sky_model.cpp と同じ左右・上下および度単位を使う。
const MIN_HORIZON_FOV: f64 = 1e-9;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum View {
    Sky,
    Horizon,
    Solar,
    Surface,
    Galaxy,
}

#[derive(Clone, Copy)]
pub struct Camera {
    pub longitude: f64,
    pub latitude: f64,
    pub zoom: f64,
}
impl Default for Camera {
    fn default() -> Self {
        Self {
            longitude: 180.0,
            latitude: 0.0,
            zoom: 1.0,
        }
    }
}
fn signed_degrees(angle: f64) -> f64 {
    (angle + 180.0).rem_euclid(360.0) - 180.0
}

impl Camera {
    /// 視線方向を中心にして、東が右の地平図または赤経が左の全天図を平面に写す。
    pub fn position(self, view: View, [lon, lat]: [f64; 2]) -> [f64; 2] {
        let dl = signed_degrees(lon - self.longitude);
        let (horizontal, vertical) = if view == View::Sky {
            (-dl, lat - self.latitude)
        } else {
            let (sine, cosine) = lat.to_radians().sin_cos();
            let (st, ct) = self.latitude.to_radians().sin_cos();
            let (sd, cd) = dl.to_radians().sin_cos();
            let forward = cosine * cd * ct + sine * st;
            let right = cosine * sd;
            let up = sine * ct - cosine * cd * st;
            (
                right.atan2(forward).to_degrees(),
                up.atan2(forward.hypot(right)).to_degrees(),
            )
        };
        [
            0.5 + horizontal * self.zoom / 360.0,
            0.5 - vertical * self.zoom / 180.0,
        ]
    }
    fn normalize(&mut self, view: View) {
        self.longitude = self.longitude.rem_euclid(360.);
        let limit = if view == View::Sky {
            90. - 90. / self.zoom
        } else {
            90.
        };
        self.latitude = self.latitude.clamp(-limit, limit);
    }
    pub fn pan(&mut self, view: View, dx: f64, dy: f64) {
        self.longitude += (if view == View::Sky { 1. } else { -1. }) * dx * 360. / self.zoom;
        self.latitude += dy * 180. / self.zoom;
        self.normalize(view);
    }
    pub fn center(&mut self, view: View, [longitude, latitude]: [f64; 2]) {
        self.zoom = self.zoom.max(4.);
        self.longitude = longitude;
        self.latitude = latitude;
        self.normalize(view);
    }
    pub fn zoom_at(&mut self, view: View, factor: f64, anchor: [f64; 2]) {
        match view {
            View::Sky => self.zoom_sky_at(factor, anchor),
            View::Horizon => {
                self.zoom_horizon_at(180. / self.zoom, factor, anchor);
            }
            View::Solar | View::Surface | View::Galaxy => {}
        }
    }
    fn zoom_sky_at(&mut self, factor: f64, anchor: [f64; 2]) {
        let previous = self.zoom;
        self.zoom = (self.zoom * factor).clamp(1., 180.);
        let change = 1. / previous - 1. / self.zoom;
        self.longitude += (0.5 - anchor[0]) * 360. * change;
        self.latitude += (0.5 - anchor[1]) * 180. * change;
        self.normalize(View::Sky);
    }

    /// 描画下限より狭い視野角も表示状態に保存するため、論理上の視野角を返す。
    fn zoom_horizon_at(&mut self, fov: f64, factor: f64, anchor: [f64; 2]) -> f64 {
        if factor == 1. {
            return fov;
        }
        let visible_fov = fov.max(MIN_HORIZON_FOV);
        let start = if factor < 1. { visible_fov } else { fov };
        let next = (start / factor).clamp(f64::from_bits(1), 180.);
        let visible_next = next.max(MIN_HORIZON_FOV);
        self.zoom = 180. / visible_next;
        if visible_next == visible_fov || anchor == [0.5, 0.5] {
            return next;
        }

        // sky_model.cpp の zoom_horizon と同じく、カーソルの視線を世界座標に戻し、
        // 拡大後の視線をその方向に一致させる方位角・仰角のうち元の姿勢に近い方を選ぶ。
        let ray = |angle: f64| {
            let [x, y] = anchor.map(|n| n.clamp(0., 1.));
            let (slon, clon) = ((x - 0.5) * 2. * angle).to_radians().sin_cos();
            let (slat, clat) = ((0.5 - y) * angle).to_radians().sin_cos();
            [clat * clon, clat * slon, slat]
        };
        let [forward, right, up] = ray(visible_fov);
        let (st, ct) = self.latitude.to_radians().sin_cos();
        let north = forward * ct - up * st;
        let vertical = forward * st + up * ct;
        let horizontal = north.hypot(right);
        let azimuth = self.longitude + right.atan2(north).to_degrees();

        let [forward, right, up] = ray(visible_next);
        if horizontal < 1e-12 || right.abs() > horizontal + 1e-12 {
            return next;
        }
        let first = (right / horizontal).clamp(-1., 1.).asin();
        let mut best = f64::INFINITY;
        let origin = (self.longitude, self.latitude);
        for offset in [first, std::f64::consts::PI - first] {
            let tilt = vertical.atan2(horizontal * offset.cos()) - up.atan2(forward);
            let latitude = signed_degrees(tilt.to_degrees());
            if latitude.abs() > 90. + 1e-10 {
                continue;
            }
            let longitude = (azimuth - offset.to_degrees()).rem_euclid(360.);
            let distance = (signed_degrees(longitude - origin.0) * ct).hypot(latitude - origin.1);
            if distance < best {
                best = distance;
                self.longitude = longitude;
                self.latitude = latitude.clamp(-90., 90.);
            }
        }
        next
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn near(a: [f64; 2], b: [f64; 2]) {
        for (a, b) in a.into_iter().zip(b) {
            assert!((a - b).abs() < 1e-10, "{a} != {b}");
        }
    }
    #[test]
    fn initial_maps_have_opposite_east_directions() {
        let camera = Camera::default();
        near(camera.position(View::Sky, [270., 45.]), [0.25, 0.25]);
        near(camera.position(View::Horizon, [270., 45.]), [0.75, 0.25]);
    }
    #[test]
    fn tilted_horizon_centers_targets_and_handles_zenith() {
        let camera = Camera {
            longitude: 350.,
            latitude: 70.,
            zoom: 1.,
        };
        near(camera.position(View::Horizon, [350., 70.]), [0.5, 0.5]);
        near(
            camera.position(View::Horizon, [90., 90.]),
            [0.5, 0.5 - 20. / 180.],
        );
    }
    #[test]
    fn sky_zoom_preserves_mouse_anchor_and_wraps_longitude() {
        let mut c = Camera::default();
        let anchor = c.position(View::Sky, [270., 45.]);
        c.zoom_at(View::Sky, 2., anchor);
        near(c.position(View::Sky, [270., 45.]), anchor);
        c.pan(View::Sky, 5., 0.);
        assert!((0. ..360.).contains(&c.longitude));
    }
    #[test]
    fn horizon_zoom_keeps_tilted_targets_under_the_pointer() {
        for (latitude, target) in [(70., [110., 55.]), (-70., [110., -55.])] {
            let mut camera = Camera {
                longitude: 73.,
                latitude,
                zoom: 2.,
            };
            let anchor = camera.position(View::Horizon, target);
            let fov = camera.zoom_horizon_at(90., 1.5, anchor);
            near(anchor, camera.position(View::Horizon, target));
            assert_eq!(fov, 60.);
            assert_eq!(camera.zoom_horizon_at(fov, 0.75, anchor), 80.);
            near(anchor, camera.position(View::Horizon, target));
        }
    }

    #[test]
    fn horizon_zoom_preserves_sub_degree_and_subpixel_fields() {
        let mut camera = Camera {
            zoom: 360.,
            ..Default::default()
        };
        let anchor = [0.5, 0.5];
        assert_eq!(camera.zoom_horizon_at(0.5, 1.5, anchor), 0.5 / 1.5);
        assert_eq!(camera.zoom, 540.);
        assert_eq!(camera.zoom_horizon_at(1e-12, 2., anchor), 5e-13);
        assert_eq!(camera.zoom, 180. / MIN_HORIZON_FOV);
        assert_eq!(
            camera.zoom_horizon_at(5e-13, 0.5, anchor),
            2. * MIN_HORIZON_FOV
        );
        assert_eq!(camera.zoom, 90. / MIN_HORIZON_FOV);
    }
}
