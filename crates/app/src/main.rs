#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod http;
mod markdown;
mod net;
mod theme;
mod ui;

use gpui_kit::{WindowOptions, prelude::*};

fn main() {
    let client = std::sync::Arc::new(http::WreqHttpClient::new());
    gpui_kit::application().with_http_client(client).run(|cx| {
        gpui_kit::init(cx);
        gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
            cx.new(|cx| ui::App::new(window, cx))
        })
        .expect("failed to open window");
    });
}
