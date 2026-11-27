use eframe::egui;
use std::time::{Duration, Instant};

#[cfg(target_os = "macos")]
mod macos;

const TIMEOUT: Duration = Duration::from_secs(5);
const BUSY: &str = "全画面の切り替え中です。完了してから切り替えてください。";
const HIDDEN: &str = "主ウィンドウを表示してから全画面を切り替えてください。";

#[derive(Default)]
struct Transition {
    pending: Option<(bool, Instant)>,
}

impl Transition {
    fn request(
        &mut self,
        current: bool,
        visible: bool,
        target: Option<bool>,
        now: Instant,
    ) -> Result<Option<bool>, &'static str> {
        if let Some((pending, _)) = self.pending {
            return if target == Some(pending) {
                Ok(None)
            } else {
                Err(BUSY)
            };
        }
        let target = target.unwrap_or(!current);
        if target == current {
            return Ok(None);
        }
        if !visible {
            return Err(HIDDEN);
        }
        self.pending = Some((target, now));
        Ok(Some(target))
    }
    fn observed(&mut self, actual: bool) {
        if self.pending.is_some_and(|(wanted, _)| wanted == actual) {
            self.pending = None;
        }
    }
    fn check(&mut self, visible: bool, now: Instant) -> Result<(), &'static str> {
        let Some((_, started)) = self.pending else {
            return Ok(());
        };
        if !visible {
            self.pending = None;
            return Err(HIDDEN);
        }
        if now.saturating_duration_since(started) >= TIMEOUT {
            self.pending = None;
            return Err("全画面の切り替え完了を確認できませんでした。");
        }
        Ok(())
    }
}

pub struct Fullscreen {
    state: Transition,
    #[cfg(target_os = "macos")]
    window: macos::Window,
}

impl Fullscreen {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Result<Self, crate::AppError> {
        Ok(Self {
            state: Transition::default(),
            #[cfg(target_os = "macos")]
            window: macos::Window::new(_cc)?,
        })
    }

    pub fn actual(&self, _ctx: &egui::Context) -> bool {
        #[cfg(target_os = "macos")]
        {
            self.window.fullscreen()
        }
        #[cfg(not(target_os = "macos"))]
        {
            _ctx.input(|i| i.viewport().fullscreen.unwrap_or(false))
        }
    }

    fn visible(&self, _ctx: &egui::Context) -> bool {
        #[cfg(target_os = "macos")]
        {
            self.window.visible()
        }
        #[cfg(not(target_os = "macos"))]
        {
            _ctx.input(|i| i.viewport().minimized != Some(true))
        }
    }

    pub fn poll(&mut self, ctx: &egui::Context) -> Result<(), &'static str> {
        let now = Instant::now();
        #[cfg(target_os = "macos")]
        for event in self.window.events() {
            match event {
                macos::Change::Completed(target) => self.state.observed(target),
                macos::Change::Started(target) => {
                    // 緑色のウィンドウボタンなど、AppKit から始まった切り替えも扱う。
                    self.state.pending.get_or_insert((target, now));
                }
            }
        }
        #[cfg(not(target_os = "macos"))]
        if let Some(actual) = ctx.input(|i| i.viewport().fullscreen) {
            self.state.observed(actual);
        }
        self.state.check(self.visible(ctx), now)?;
        if self.state.pending.is_some() {
            ctx.request_repaint_after(Duration::from_millis(30));
        }
        Ok(())
    }

    pub fn request(
        &mut self,
        ctx: &egui::Context,
        target: Option<bool>,
    ) -> Result<(), &'static str> {
        if let Some(target) =
            self.state
                .request(self.actual(ctx), self.visible(ctx), target, Instant::now())?
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(target));
            ctx.request_repaint();
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicate_opposite_and_toggle_wait_for_native_completion() {
        let now = Instant::now();
        let mut t = Transition::default();
        assert_eq!(t.request(false, true, Some(true), now), Ok(Some(true)));
        assert_eq!(t.request(false, true, Some(true), now), Ok(None));
        assert_eq!(t.request(true, true, Some(false), now), Err(BUSY));
        assert_eq!(t.request(true, true, None, now), Err(BUSY));
        t.observed(false);
        assert!(t.pending.is_some());
        t.observed(true);
        assert_eq!(t.request(true, true, Some(true), now), Ok(None));
        assert_eq!(t.request(true, true, None, now), Ok(Some(false)));
        t.observed(false);
        assert!(t.pending.is_none());
    }

    #[test]
    fn unavailable_window_and_timeout_release_pending_requests_for_retry() {
        let now = Instant::now();
        let mut t = Transition::default();
        assert_eq!(t.request(false, false, None, now), Err(HIDDEN));
        assert!(t.pending.is_none());
        assert_eq!(t.request(false, true, None, now), Ok(Some(true)));
        assert_eq!(t.check(false, now), Err(HIDDEN));
        assert_eq!(t.request(false, true, None, now), Ok(Some(true)));
        assert!(t
            .check(true, now + TIMEOUT - Duration::from_millis(1))
            .is_ok());
        assert!(t.check(true, now + TIMEOUT).is_err());
        assert_eq!(t.request(false, true, None, now + TIMEOUT), Ok(Some(true)));
    }
}
