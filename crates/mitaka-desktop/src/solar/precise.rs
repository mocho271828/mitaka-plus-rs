//! Mitaka_plus f9fe624 shared_camera.cpp / solar_camera.cpp の pc基準点・AU残差。
use super::{Point, SolarCamera};

pub(crate) const AU_PER_PC: f64 = 648000. / std::f64::consts::PI;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Target {
    pub anchor_pc: Point,
    pub offset_au: Point,
}
impl Target {
    pub(crate) fn relative_pc(self, position: Point) -> Point {
        std::array::from_fn(|i| (position[i] - self.anchor_pc[i]) - self.offset_au[i] / AU_PER_PC)
    }
    pub fn approximate(self) -> Point {
        std::array::from_fn(|i| self.anchor_pc[i].mul_add(AU_PER_PC, self.offset_au[i]))
    }
    fn from_au(p: Point) -> Self {
        let anchor_pc = p.map(|v| v / AU_PER_PC);
        Self {
            anchor_pc,
            offset_au: std::array::from_fn(|i| (-anchor_pc[i]).mul_add(AU_PER_PC, p[i])),
        }
    }
    fn normalized(mut self, distance: f64) -> Self {
        for (anchor, offset) in self.anchor_pc.iter_mut().zip(&mut self.offset_au) {
            if offset.abs() / distance <= 1e6 {
                continue;
            }
            let shift = *offset / AU_PER_PC;
            let next = *anchor + shift;
            if next.abs() > 1e40 {
                continue;
            }
            let virtual_shift = next - *anchor;
            let rounding = (*anchor - (next - virtual_shift)) + (shift - virtual_shift);
            *offset = rounding.mul_add(AU_PER_PC, (-shift).mul_add(AU_PER_PC, *offset));
            *anchor = next;
        }
        self
    }
}
impl SolarCamera {
    pub(crate) fn orbit(&mut self, dx: f64, dy: f64) {
        use glam::{DQuat, DVec3};
        let [mut right, up, mut outward] = self.basis().map(DVec3::from_array);
        let horizontal = DQuat::from_axis_angle(up, (-dx * 0.35).to_radians());
        right = horizontal * right;
        outward = DQuat::from_axis_angle(right, (-dy * 0.35).to_radians()) * (horizontal * outward);
        let up = outward.cross(right);
        if self.frame.is_some() {
            self.frame = Some([right, up, outward].map(|v| v.to_array()));
        } else {
            self.yaw = outward.y.atan2(outward.x).to_degrees();
            self.pitch = outward
                .z
                .clamp(-1., 1.)
                .asin()
                .to_degrees()
                .clamp(-89., 89.);
            self.roll = 0.;
            let [r, u, _] = self.basis().map(DVec3::from_array);
            self.roll = right.dot(u).atan2(right.dot(r)).to_degrees();
        }
    }
    pub(crate) fn roll_by(&mut self, degrees: f64) {
        use glam::{DQuat, DVec3};
        if self.frame.is_some() {
            let [right, _, out] = self.basis().map(DVec3::from_array);
            let r = DQuat::from_axis_angle(out, degrees.to_radians()) * right;
            self.frame = Some([r, out.cross(r), out].map(|v| v.to_array()));
        } else {
            self.roll = (self.roll + degrees + 180.).rem_euclid(360.) - 180.;
        }
    }
    pub(crate) fn precise(self) -> Option<Target> {
        self.precise_target
            .filter(|p| p.approximate() == self.target)
    }
    pub(crate) fn anchor(self) -> Target {
        self.precise()
            .unwrap_or_else(|| Target::from_au(self.target))
            .normalized(self.distance)
    }
    pub(crate) fn set_anchor(&mut self, target: Target) {
        self.target = target.approximate();
        self.precise_target = Some(target);
    }
    pub(crate) fn set_target_pc(&mut self, anchor_pc: Point) {
        self.set_anchor(Target {
            anchor_pc,
            offset_au: [0.; 3],
        });
    }
    pub(crate) fn pan_au(&mut self, delta: Point) {
        let mut p = self.anchor();
        if p.anchor_pc.iter().any(|a| a.abs() > 1e40) {
            self.target = super::plus(self.target, delta);
            self.precise_target = None;
            return;
        }
        p.offset_au = super::plus(p.offset_au, delta);
        self.set_anchor(p);
    }
    pub(crate) fn relative_au(self, position: Point) -> Point {
        match self.precise() {
            Some(target) => {
                let p = target.normalized(self.distance);
                std::array::from_fn(|i| {
                    (-p.anchor_pc[i]).mul_add(AU_PER_PC, position[i]) - p.offset_au[i]
                })
            }
            None => super::minus(position, self.target),
        }
    }
}
