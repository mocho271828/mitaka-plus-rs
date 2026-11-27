use block2::RcBlock;
use eframe::egui;
use objc2::{rc::Retained, runtime::ProtocolObject};
use objc2_app_kit::{
    NSView, NSWindow, NSWindowDidEnterFullScreenNotification,
    NSWindowDidExitFullScreenNotification, NSWindowStyleMask,
    NSWindowWillEnterFullScreenNotification, NSWindowWillExitFullScreenNotification,
};
use objc2_foundation::{NSNotification, NSNotificationCenter, NSObjectProtocol};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::{ptr::NonNull, sync::mpsc};

#[derive(Clone, Copy)]
pub(super) enum Change {
    Started(bool),
    Completed(bool),
}

pub(super) struct Window {
    handle: Retained<NSWindow>,
    center: Retained<NSNotificationCenter>,
    observers: Vec<Retained<ProtocolObject<dyn NSObjectProtocol>>>,
    changes: mpsc::Receiver<Change>,
}
impl Window {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Result<Self, crate::AppError> {
        let RawWindowHandle::AppKit(handle) = cc.window_handle()?.as_raw() else {
            return Err("AppKit の主ウィンドウを取得できません".into());
        };
        // eframe が生成した生存中の主 NSView をメインスレッドで参照し、所有ウィンドウを保持する。
        let view = unsafe { handle.ns_view.cast::<NSView>().as_ref() };
        let handle = view.window().ok_or("主 NSWindow を取得できません")?;
        let center = NSNotificationCenter::defaultCenter();
        let (send, changes) = mpsc::channel();
        let mut observers = Vec::new();
        // winit の fullscreen() はアニメーション開始時に目標値へ変わるため、
        // 反対方向の要求を受け付ける時点は、対象ウィンドウの DidEnter/DidExit で判定する。
        let notices = unsafe {
            [
                (
                    NSWindowWillEnterFullScreenNotification,
                    Change::Started(true),
                ),
                (
                    NSWindowWillExitFullScreenNotification,
                    Change::Started(false),
                ),
                (
                    NSWindowDidEnterFullScreenNotification,
                    Change::Completed(true),
                ),
                (
                    NSWindowDidExitFullScreenNotification,
                    Change::Completed(false),
                ),
            ]
        };
        for (notice, change) in notices {
            let send = send.clone();
            let ctx: egui::Context = cc.egui_ctx.clone();
            let block = RcBlock::new(move |_: NonNull<NSNotification>| {
                let _ = send.send(change);
                ctx.request_repaint();
            });
            // AppKit の通知元を限定する。コールバックが取得する値はいずれも Send + Sync。
            let token = unsafe {
                center.addObserverForName_object_queue_usingBlock(
                    Some(notice),
                    Some(&handle),
                    None,
                    &block,
                )
            };
            observers.push(token);
        }
        Ok(Self {
            handle,
            center,
            observers,
            changes,
        })
    }
    pub fn fullscreen(&self) -> bool {
        self.handle
            .styleMask()
            .contains(NSWindowStyleMask::FullScreen)
    }
    pub fn visible(&self) -> bool {
        self.handle.isVisible() && !self.handle.isMiniaturized()
    }
    pub fn events(&self) -> impl Iterator<Item = Change> + '_ {
        self.changes.try_iter()
    }
}
impl Drop for Window {
    fn drop(&mut self) {
        for observer in &self.observers {
            // 登録時に返されたトークンだけを同じ通知センターから解除する。
            unsafe { self.center.removeObserver(observer.as_ref()) };
        }
    }
}
