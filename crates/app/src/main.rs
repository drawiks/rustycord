#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod assets;
mod http;
mod markdown;
mod net;
mod theme;
mod ui;

use std::{borrow::Cow, sync::Arc};

use gpui_kit::{Bounds, TitlebarOptions, WindowBounds, WindowOptions, point, prelude::*, px, size};

fn main() {
    let _logs = init_logging();
    let client = Arc::new(http::WreqHttpClient::new());
    gpui_kit::application()
        .with_assets(assets::AppAssets)
        .with_http_client(client)
        .run(|cx| {
            gpui_kit::init(cx);
            cx.text_system()
                .add_fonts(vec![
                    Cow::Borrowed(include_bytes!("../assets/fonts/Inter.ttf").as_slice()),
                    Cow::Borrowed(include_bytes!("../assets/fonts/JetBrainsMono.ttf").as_slice()),
                ])
                .expect("failed to load bundled fonts");
            theme::apply(cx);
            gpui_kit::open_window(window_options(), cx, |window, cx| {
                cx.new(|cx| ui::App::new(window, cx))
            })
            .expect("failed to open window");
        });
}

fn window_options() -> WindowOptions {
    let icon = image::load_from_memory(include_bytes!("../assets/icon.png"))
        .ok()
        .map(|image| Arc::new(image.to_rgba8()));

    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::new(
            point(px(80.), px(80.)),
            size(px(1280.), px(820.)),
        ))),
        titlebar: Some(TitlebarOptions {
            title: Some("rustycord".into()),
            appears_transparent: false,
            traffic_light_position: None,
        }),
        window_min_size: Some(size(px(900.), px(600.))),
        icon,
        ..Default::default()
    }
}

fn init_logging() -> tracing_appender::non_blocking::WorkerGuard {
    use tracing_subscriber::EnvFilter;

    let dir = dirs::data_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("rustycord");
    let _ = std::fs::create_dir_all(&dir);
    let file = tracing_appender::rolling::never(dir, "rustycord.log");
    let (writer, guard) = tracing_appender::non_blocking(file);
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(writer)
        .with_ansi(false)
        .init();
    guard
}
