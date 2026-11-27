//! Mitaka_plus f9fe624 SkyView.mm / HorizonView.mm のスクロールとピンチ。
use eframe::egui::{Event, InputState, MouseWheelUnit, Vec2};

pub(super) fn gesture(input: &InputState) -> (Vec2, f32) {
    let mut pan = Vec2::ZERO;
    let mut wheel = 0.;
    for event in &input.events {
        if let Event::MouseWheel {
            unit,
            delta,
            modifiers,
        } = event
        {
            // egui は修飾キーつきホイールを zoom_delta に含める。
            if modifiers.ctrl || modifiers.mac_cmd || modifiers.command {
                continue;
            }
            match unit {
                MouseWheelUnit::Point => pan += *delta,
                MouseWheelUnit::Line | MouseWheelUnit::Page => wheel += delta.y,
            }
        }
    }
    (
        pan,
        (wheel * 0.08).clamp(-2., 2.).exp() * input.zoom_delta(),
    )
}

pub(super) fn focus_target(
    picking: crate::viewer::Picking,
    double_clicked: bool,
    coords: Option<[f64; 2]>,
) -> Option<[f64; 2]> {
    coords.filter(|_| picking == crate::viewer::Picking::Click && double_clicked)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::projection::{Camera, View};
    use eframe::egui::{self, Context, Modifiers, RawInput};

    #[test]
    fn only_double_click_on_visible_selection_centers_the_sky_and_horizon() {
        use crate::viewer::Picking;
        for view in [View::Sky, View::Horizon] {
            let mut camera = Camera::default();
            let target = [160., 20.];
            assert_eq!(focus_target(Picking::Hover, true, Some(target)), None);
            assert_eq!(focus_target(Picking::Click, false, Some(target)), None);
            assert_eq!(focus_target(Picking::Click, true, None), None);
            camera.center(
                view,
                focus_target(Picking::Click, true, Some(target)).unwrap(),
            );
            assert_eq!(camera.zoom, 4.);
            assert_eq!(camera.position(view, target), [0.5, 0.5]);
        }
    }

    #[test]
    fn precision_scroll_pans_both_axes_and_discrete_wheel_and_pinch_zoom() {
        for view in [View::Sky, View::Horizon] {
            let mut camera = Camera {
                zoom: 4.,
                ..Default::default()
            };
            let ctx = Context::default();
            let events = vec![
                Event::MouseWheel {
                    unit: MouseWheelUnit::Point,
                    delta: egui::vec2(20., 10.),
                    modifiers: Modifiers::NONE,
                },
                Event::MouseWheel {
                    unit: MouseWheelUnit::Line,
                    delta: egui::vec2(0., 1.),
                    modifiers: Modifiers::NONE,
                },
                Event::Zoom(1.25),
            ];
            let _ = ctx.run(
                RawInput {
                    events,
                    ..Default::default()
                },
                |ctx| {
                    let (pan, zoom) = ctx.input(gesture);
                    assert_eq!(pan, egui::vec2(20., 10.));
                    assert!((zoom - 0.08_f32.exp() * 1.25).abs() < 1e-6);
                    camera.pan(view, f64::from(pan.x / 100.), f64::from(pan.y / 100.));
                    assert_eq!(camera.zoom, 4.);
                    let east = view == View::Sky;
                    assert_eq!(camera.longitude > 180., east);
                    assert!(camera.latitude > 0.);
                },
            );
        }
    }
}
