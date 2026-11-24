#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod net;
mod ui;

use gpui_kit::{WindowOptions, prelude::*};

fn main() {
    gpui_kit::application().run(|cx| {
        gpui_kit::init(cx);
        gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
            cx.new(|cx| ui::App::new(window, cx))
        })
        .expect("failed to open window");
    });
}
