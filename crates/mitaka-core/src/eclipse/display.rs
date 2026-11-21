//! Mitaka_plus f9fe624 eclipse_surface.cpp / eclipse_cone_renderer.cpp. Physical values are in AU.
use super::{
    Cone, ShadowCones, Sphere, Vector3, cross, dot, invalid, length, occluder_cones, shadow_cones,
    shadow_multiplier, sub, surface_sample,
};
use std::io;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Options {
    pub shadows: bool,
    pub borders: bool,
    pub umbra: bool,
    pub penumbra: bool,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            shadows: true,
            borders: false,
            umbra: false,
            penumbra: false,
        }
    }
}
#[derive(Debug)]
pub struct Surface {
    sun: Sphere,
    receiver: Sphere,
    casters: Vec<(Sphere, ShadowCones)>,
    options: Options,
    color: [f64; 3],
}
#[derive(Debug, PartialEq)]
pub struct Effect {
    pub multiplier: [f64; 3],
    pub border_rgb: Option<[f64; 3]>,
}
impl Default for Effect {
    fn default() -> Self {
        Self {
            multiplier: [1.; 3],
            border_rgb: None,
        }
    }
}
fn norm(a: Vector3) -> f64 {
    a[0].hypot(a[1]).hypot(a[2])
}
fn scale(a: Vector3, n: f64) -> Vector3 {
    a.map(|v| v * n)
}
fn add(a: Vector3, b: Vector3) -> Vector3 {
    std::array::from_fn(|i| a[i] + b[i])
}
fn on_border(cone: Cone, point: Vector3, footprint: f64) -> bool {
    let v = sub(point, cone.apex_au);
    let axial = dot(v, cone.axis);
    axial >= 0.
        && (norm(cross(v, cone.axis)) * cone.cos_half_angle - axial * cone.sin_half_angle).abs()
            <= footprint
}
impl Surface {
    pub fn bind(
        sun: Sphere,
        receiver: Sphere,
        casters: &[Sphere],
        options: Options,
        color: [f64; 3],
    ) -> io::Result<Option<Self>> {
        shadow_multiplier(0., color)?;
        if !options.shadows && !options.borders {
            return Ok(None);
        }
        let mut sources = Vec::with_capacity(casters.len());
        let mut active = false;
        for &caster in casters {
            if let Some(cones) = shadow_cones(sun, caster, receiver)? {
                active = true;
                sources.push((caster, cones));
            } else {
                sources.push((caster, occluder_cones(sun, caster)?));
            }
        }
        Ok(active.then_some(Self {
            sun,
            receiver,
            casters: sources,
            options,
            color,
        }))
    }
    pub fn effect(&self, normal: Vector3, footprint: f64) -> io::Result<Effect> {
        if !normal.iter().all(|v| v.is_finite())
            || (dot(normal, normal) - 1.).abs() > 1e-8
            || !footprint.is_finite()
            || footprint < 0.
        {
            return Err(invalid(
                "eclipse requires a unit normal and nonnegative finite pixel footprint",
            ));
        }
        let point = add(
            self.receiver.center_au,
            scale(normal, self.receiver.radius_au),
        );
        let mut result = Effect::default();
        if dot(normal, sub(self.sun.center_au, point)) <= 0. {
            return Ok(result);
        }
        for &(caster, cones) in &self.casters {
            if self.options.shadows {
                let mult = shadow_multiplier(
                    surface_sample(self.sun, caster, point)?.obscured_fraction,
                    self.color,
                )?;
                for (c, m) in result.multiplier.iter_mut().zip(mult) {
                    *c *= m;
                }
            }
            if self.options.borders {
                if on_border(cones.penumbra, point, footprint) {
                    result.border_rgb = Some([1., 0., 0.]);
                }
                if on_border(cones.umbra, point, footprint) {
                    result.border_rgb = Some([1., 0.5, 0.]);
                }
            }
        }
        Ok(result)
    }
}
#[derive(Clone, Copy, Debug)]
struct Clip {
    receiver: Vector3,
    radius: f64,
    replacement: f64,
}
#[derive(Clone, Copy, Debug)]
pub struct DisplayCone {
    pub apex: Vector3,
    pub axis: Vector3,
    pub slope: f64,
    pub near: f64,
    pub far: f64,
    far_alpha: f64,
    clip: Option<Clip>,
}
pub fn display_cones(
    sun: Sphere,
    occ: Sphere,
    receiver: Sphere,
    options: Options,
) -> io::Result<Vec<DisplayCone>> {
    let physical = occluder_cones(sun, occ)?;
    super::separated(sun, receiver)?;
    super::separated(occ, receiver)?;
    let r = sub(receiver.center_au, sun.center_au);
    let rd = length(r)?;
    let d = length(sub(occ.center_au, sun.center_au))?;
    let ul = length(sub(physical.umbra.apex_au, sun.center_au))?;
    let pl = length(sub(physical.penumbra.apex_au, sun.center_au))?;
    let mut out = Vec::with_capacity(3);
    if options.umbra {
        let ratio = sun.radius_au / d;
        let far = ul - sun.radius_au * ratio;
        let q = 1. - rd / ul;
        let along = dot(r, physical.axis);
        let near = if (0. ..1.).contains(&q)
            && along >= 0.
            && norm(sub(r, scale(physical.axis, along))) < receiver.radius_au
        {
            q * far
        } else {
            0.
        };
        out.push(DisplayCone {
            apex: physical.umbra.apex_au,
            axis: physical.umbra.axis,
            slope: sun.radius_au * (1. - ratio * ratio).sqrt() / far,
            near,
            far,
            far_alpha: 0.,
            clip: None,
        });
    }
    if options.penumbra {
        let ratio = sun.radius_au / pl;
        if !(0. ..1.).contains(&ratio) || ratio == 0. {
            return Err(invalid("invalid eclipse penumbra tangent"));
        }
        let slope = ratio / (1. - ratio * ratio).sqrt();
        let apex = physical.penumbra.apex_au;
        let axis = physical.penumbra.axis;
        out.push(DisplayCone {
            apex,
            axis: scale(axis, -1.),
            slope,
            near: 0.,
            far: pl - sun.radius_au * ratio,
            far_alpha: 0.,
            clip: None,
        });
        out.push(DisplayCone {
            apex,
            axis,
            slope,
            near: 0.,
            far: ul.max(rd) - pl,
            far_alpha: 0.5,
            clip: Some(Clip {
                receiver: sub(receiver.center_au, apex),
                radius: receiver.radius_au,
                replacement: rd - pl,
            }),
        });
    }
    if out.iter().any(|c| {
        !c.slope.is_finite()
            || c.slope <= 0.
            || !c.far.is_finite()
            || !(0. ..c.far).contains(&c.near)
    }) {
        return Err(invalid("invalid eclipse display cone extent"));
    }
    Ok(out)
}
impl DisplayCone {
    fn end(self, clip: Clip, radial: Vector3) -> f64 {
        let n = 1_f64.hypot(self.slope);
        let generator = add(scale(self.axis, 1. / n), scale(radial, self.slope / n));
        let along = dot(clip.receiver, generator);
        if along >= 0. && norm(sub(clip.receiver, scale(generator, along))) < clip.radius {
            clip.replacement
        } else {
            self.far
        }
    }
    /// Returns (distance along camera ray in AU, SourceOver opacity) at each finite sheet hit.
    pub fn intersections(self, eye_from_apex: Vector3, ray: Vector3) -> [Option<(f64, f64)>; 2] {
        let eye = scale(eye_from_apex, 1. / self.far);
        let k = 1. + self.slope * self.slope;
        let axial = dot(eye, self.axis);
        let along = dot(ray, self.axis);
        let a = 1. - k * along * along;
        let b = 2. * (dot(eye, ray) - k * axial * along);
        let c = dot(eye, eye) - k * axial * axial;
        let disc = b * b - 4. * a * c;
        let root = if a == 0. {
            [if b == 0. { f64::NAN } else { -c / b }, f64::NAN]
        } else if disc < 0. || !disc.is_finite() {
            return [None; 2];
        } else if disc == 0. {
            [-b / (2. * a), f64::NAN]
        } else {
            let q = -0.5 * (b + disc.sqrt().copysign(b));
            [q / a, c / q]
        };
        root.map(|t| {
            if !t.is_finite() || t <= 0. {
                return None;
            }
            let z = axial + t * along;
            let (near, far) = if let Some(clip) = self.clip {
                let mut end = self.far;
                if z != 0. {
                    let radial = sub(add(eye, scale(ray, t)), scale(self.axis, z));
                    let n = norm(radial);
                    if n <= 0. {
                        return None;
                    }
                    end = self.end(clip, scale(radial, 1_f64.copysign(z) / n));
                }
                (0_f64.min(end), 0_f64.max(end))
            } else {
                (self.near, self.far)
            };
            if near == far || z < near / self.far || z > far / self.far {
                return None;
            }
            let weight = ((z * self.far - self.near) / (self.far - self.near)).clamp(0., 1.);
            Some((t * self.far, 0.5 + (self.far_alpha - 0.5) * weight))
        })
    }
}
