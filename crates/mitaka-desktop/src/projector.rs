use eframe::egui::{
    self, emath::TSTransform, Color32, Context, LayerId, Rect, ViewportBuilder, ViewportId,
};

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

pub struct Projector {
    primary_event: Arc<std::sync::Mutex<Option<egui::Event>>>,
    closed: Arc<AtomicBool>,
    pub open: bool,
    pub controls: bool,
    pub probe: bool,
    verified: Arc<AtomicBool>,
    pointer: bool,
    gain: f32,
}
impl Default for Projector {
    fn default() -> Self {
        Self {
            open: false,
            primary_event: Default::default(),
            controls: false,
            probe: false,
            verified: Default::default(),
            closed: Default::default(),
            pointer: false,
            gain: 1.,
        }
    }
}
fn fit(src: Rect, dst: Rect) -> TSTransform {
    let scale = (dst.width() / src.width()).min(dst.height() / src.height());
    TSTransform::new(
        dst.center().to_vec2() - src.center().to_vec2() * scale,
        scale,
    )
}
impl Projector {
    pub fn verified(&self) -> bool {
        self.verified.load(Ordering::Relaxed)
    }
    pub fn settings(&mut self, ctx: &Context) {
        egui::Window::new("投影設定")
            .open(&mut self.controls)
            .show(ctx, |ui| {
                ui.checkbox(&mut self.open, "投影ウィンドウを開く");
                ui.checkbox(&mut self.pointer, "マウスポインターを投影");
                ui.add(egui::Slider::new(&mut self.gain, 0. ..=1.).text("投影の明るさ"));
            });
    }
    pub fn frame(&mut self, ctx: &Context, layer: LayerId, rect: Rect) {
        if let Some(event) = self.primary_event.lock().unwrap().take() {
            ctx.input_mut(|i| i.events.push(event));
        }
        if self.closed.swap(false, Ordering::Relaxed) {
            self.open = false;
        }
        if !self.open || !rect.is_positive() {
            return;
        }
        // 星図を描いたレイヤーだけを複製し、メニューやスクリプト入力を投影画面に含めない。
        let shapes = ctx.graphics(|g| {
            g.get(layer)
                .map(|p| p.all_entries().cloned().collect::<Vec<_>>())
                .unwrap_or_default()
        });
        let pointer = ctx
            .pointer_hover_pos()
            .filter(|p| rect.contains(*p) && self.pointer);
        let id = ViewportId::from_hash_of("mitaka-projector");
        let probe = self.probe && !self.verified();
        let verified = self.verified.clone();
        let closed = self.closed.clone();
        let forwarded = self.primary_event.clone();
        let gain = self.gain;
        if probe {
            check_capture(ctx, id, &verified);
        }
        ctx.request_repaint_of(id);
        ctx.show_viewport_deferred(
            id,
            ViewportBuilder::default()
                .with_title("Mitaka Plus — 投影")
                .with_inner_size([800., 520.]),
            move |child, class| {
                // wgpu は先に再描画したウィンドウへスクリーンショットの完了を配信する。
                // 本体向けの画像を、投影側が先に受け取った場合も本体に引き渡す。
                let from_root=child.input(|i|i.events.iter().find(|e| matches!(e, egui::Event::Screenshot{viewport_id,..} if *viewport_id==ViewportId::ROOT)).cloned());
                if let Some(e)=from_root {
                    *forwarded.lock().unwrap()=Some(e);
                    child.request_repaint_of(ViewportId::ROOT);
                }

                if class == egui::ViewportClass::Embedded
                    || child.input(|i| i.viewport().close_requested())
                {
                    closed.store(true, Ordering::Relaxed);
                    child.request_repaint_of(ViewportId::ROOT);
                    return;
                }
                if probe {
                    check_capture(child, id, &verified);
                    if !verified.load(Ordering::Relaxed) {
                        child.send_viewport_cmd(egui::ViewportCommand::Screenshot(
                            Default::default(),
                        ));
                        child.request_repaint_of(ViewportId::ROOT);
                    }
                }
                egui::CentralPanel::default()
                    .frame(egui::Frame::NONE.fill(Color32::BLACK))
                    .show(child, |ui| {
                        let destination = ui.max_rect();
                        if !destination.is_positive() {
                            return;
                        }
                        let transform = fit(rect, destination);
                        for mut entry in shapes.iter().cloned() {
                            let clip = transform
                                .mul_rect(entry.clip_rect.intersect(rect))
                                .intersect(destination);
                            if !clip.is_positive() {
                                continue;
                            }
                            entry.shape.transform(transform);
                            ui.painter().with_clip_rect(clip).add(entry.shape);
                        }
                        if let Some(p) = pointer.map(|p| transform.mul_pos(p)) {
                            ui.painter().circle_stroke(
                                p,
                                9.,
                                egui::Stroke::new(2_f32, Color32::RED),
                            );
                        }
                        if gain < 1. {
                            ui.painter().rect_filled(
                                destination,
                                0.,
                                Color32::from_black_alpha(((1. - gain) * 255.).round() as u8),
                            );
                        }
                    });
            },
        );
    }
}
fn check_capture(ctx: &Context, id: ViewportId, verified: &AtomicBool) {
    ctx.input(|i| {
        for event in &i.events {
            if let egui::Event::Screenshot {
                viewport_id, image, ..
            } = event
            {
                if *viewport_id == id {
                    let colors: std::collections::HashSet<_> =
                        image.pixels.iter().step_by(11).copied().collect();
                    if colors.len() > 32 && !verified.swap(true, Ordering::Relaxed) {
                        println!(
                            "projector pixels={}x{} colors={}",
                            image.width(),
                            image.height(),
                            colors.len()
                        );
                    }
                }
            }
        }
    });
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn letterboxes_shape_and_pointer_together() {
        let src = Rect::from_min_size(egui::pos2(4., 12.), egui::vec2(200., 100.));
        let dst = Rect::from_min_size(egui::pos2(5., 0.), egui::vec2(400., 400.));
        let t = fit(src, dst);
        assert_eq!(t.mul_pos(src.center()), dst.center());
        assert_eq!(t.mul_pos(src.min), egui::pos2(5., 100.));
    }
}
