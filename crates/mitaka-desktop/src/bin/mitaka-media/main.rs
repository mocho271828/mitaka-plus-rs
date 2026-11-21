mod control;
mod frames;
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
    let first = args.next().ok_or("usage: mitaka-media FILE")?;
    let inherited = first == "--selected-file";
    let path = PathBuf::from(if inherited {
        args.next().ok_or("missing media filename")?
    } else {
        first
    });
    if args.next().is_some() {
        return Err("unexpected argument".into());
    }
    let player = if inherited {
        #[cfg(unix)]
        let owned = {
            use std::os::fd::AsFd;
            std::io::stdin().as_fd().try_clone_to_owned()?
        };
        #[cfg(windows)]
        let owned = {
            use std::os::windows::io::AsHandle;
            std::io::stdin().as_handle().try_clone_to_owned()?
        };
        stream::Player::from_handle(std::fs::File::from(owned), &path)?
    } else {
        stream::Player::open(&path)?
    };
    println!("{}", player.native);
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
        .with_autoplay(true)
        .with_devtools(false)
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
