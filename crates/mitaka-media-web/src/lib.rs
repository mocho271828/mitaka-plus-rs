//! 配布物に同梱する固定メディア画面の制御。URL・DOM・コマンドをユーザーファイルから構築しない。
#[cfg(target_arch = "wasm32")]
mod web {
    use std::{cell::Cell, rc::Rc};
    use wasm_bindgen::{JsCast, prelude::*};
    use wasm_bindgen_futures::{JsFuture, spawn_local};
    use web_sys::{HtmlMediaElement, RequestInit, Response, Window};

    async fn exchange(window: Window, e: HtmlMediaElement) -> Result<(), JsValue> {
        let duration = e.duration();
        let t = e.current_time();
        let state = format!(
            "{} {} {} {}",
            if t.is_finite() { t } else { 0. },
            if duration.is_finite() { duration } else { -1. },
            u8::from(!e.paused() && !e.ended()),
            e.ready_state()
        );
        let init = RequestInit::new();
        init.set_method("POST");
        init.set_body(&JsValue::from(state));
        let reply: Response = JsFuture::from(window.fetch_with_str_and_init("sync", &init))
            .await?
            .dyn_into()?;
        if !reply.ok() {
            return Err("player control disconnected".into());
        }
        let command = JsFuture::from(reply.text()?)
            .await?
            .as_string()
            .unwrap_or_default();
        match command.as_str() {
            "play" => {
                JsFuture::from(e.play()?).await?;
            }
            "pause" => e.pause()?,
            command => {
                if let Some(second) = command
                    .strip_prefix("seek ")
                    .and_then(|n| n.parse::<f64>().ok())
                    .filter(|n| n.is_finite() && *n >= 0.)
                {
                    e.set_current_time(second.min(duration));
                }
            }
        }
        Ok(())
    }
    #[wasm_bindgen(start)]
    pub fn start() -> Result<(), JsValue> {
        let window = web_sys::window().ok_or("window missing")?;
        let media: HtmlMediaElement = window
            .document()
            .ok_or("document missing")?
            .get_element_by_id("player")
            .ok_or("media missing")?
            .dyn_into()?;
        let busy = Rc::new(Cell::new(false));
        let w = window.clone();
        let tick = Closure::<dyn FnMut()>::new(move || {
            if busy.replace(true) {
                return;
            }
            let busy = busy.clone();
            let window = w.clone();
            let e = media.clone();
            spawn_local(async move {
                let _ = exchange(window, e).await;
                busy.set(false);
            });
        });
        window.set_interval_with_callback_and_timeout_and_arguments_0(
            tick.as_ref().unchecked_ref(),
            125,
        )?;
        tick.forget();
        Ok(())
    }
}
