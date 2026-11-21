mod stream;

use std::{env, error::Error, path::PathBuf};
use tao::{
    dpi::LogicalSize,
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
    window::WindowBuilder,
};
use wry::{NewWindowResponse, WebViewBuilder};

fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    let mut args = env::args_os().skip(1);
    let path = args.next().ok_or("Usage: mitaka-media FILE")?;
    if args.next().is_some() {
        return Err("expected one local file".into());
    }
    let path = PathBuf::from(path);
    let player = stream::Player::open(&path)?;
    let url = player.url.clone();
    let event_loop = EventLoop::new();
    let window = WindowBuilder::new()
        .with_title(
            path.file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("メディア"),
        )
        .with_inner_size(LogicalSize::new(800., 500.))
        .build(&event_loop)?;
    let allowed = url.clone();
    let builder = WebViewBuilder::new()
        .with_incognito(true)
        .with_autoplay(false)
        .with_devtools(false)
        .with_javascript_disabled()
        .with_navigation_handler(move |destination| destination == allowed)
        .with_new_window_req_handler(|_, _| NewWindowResponse::Deny)
        .with_url(url);
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    let webview = builder.build(&window)?;
    #[cfg(target_os = "linux")]
    let webview = {
        use tao::platform::unix::WindowExtUnix;
        use wry::WebViewBuilderExtUnix;
        builder.build_gtk(
            window
                .default_vbox()
                .ok_or("GTK window container unavailable")?,
        )?
    };
    event_loop.run(move |event, _, flow| {
        let _keep = (&player, &webview, &window);
        *flow = ControlFlow::Wait;
        if matches!(
            event,
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            }
        ) {
            *flow = ControlFlow::Exit;
        }
    })
}
