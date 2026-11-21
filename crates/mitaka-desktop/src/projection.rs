//! 円筒投影。現行版の sky_model.cpp と同じ左右・上下および度単位を使う。
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
        let previous = self.zoom;
        self.zoom = (self.zoom * factor).clamp(1., 180.);
        let change = 1. / previous - 1. / self.zoom;
        self.longitude +=
            (if view == View::Sky { -1. } else { 1. }) * (anchor[0] - 0.5) * 360. * change;
        self.latitude += (0.5 - anchor[1]) * 180. * change;
        self.normalize(view);
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
}
