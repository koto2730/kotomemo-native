#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod attachments;
mod clipboard;
mod editor_state;
mod entity;
mod finder_state;
mod fonts;
mod io;
mod ipc;
mod ui;
mod usecase;
mod secure_tokens;

use app::KotomemoApp;
use std::path::PathBuf;
use std::sync::Arc;

fn load_icon() -> egui::IconData {
    let bytes = include_bytes!("../icon-source.png");
    let image = image::load_from_memory(bytes).expect("bundled icon must decode").to_rgba8();
    let (width, height) = image.dimensions();
    egui::IconData {
        rgba: image.into_raw(),
        width,
        height,
    }
}

fn main() -> eframe::Result {
    // CLI-arg files (e.g. `kotomemo path/to/file.txt`, or "Open
    // with" registration): absolute-ify each and hand them to the app so
    // it opens (or prepares an empty buffer targeting) them at startup.
    let cwd = std::env::current_dir().unwrap_or_default();
    let initial_paths: Vec<PathBuf> = std::env::args()
        .skip(1)
        .map(|arg| {
            let path = PathBuf::from(&arg);
            // `Path::to_absolute()`-equivalent: resolve against CWD without
            // requiring the file to exist yet (a CLI arg naming a
            // not-yet-created file is a valid "prepare this as the save
            // target" case) and without resolving symlinks.
            if path.is_absolute() { path } else { cwd.join(path) }
        })
        .collect();

    let Some(primary_ipc) = ipc::try_acquire_primary_or_forward(&initial_paths) else {
        // Another instance is already running and received `initial_paths`.
        return Ok(());
    };

    let icon = Arc::new(load_icon());
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([900.0, 700.0])
            .with_title("kotomemo")
            .with_icon(Arc::clone(&icon)),
        ..Default::default()
    };
    eframe::run_native(
        "kotomemo",
        native_options,
        Box::new(move |cc| {
            fonts::setup(&cc.egui_ctx);
            primary_ipc.set_egui_ctx(cc.egui_ctx.clone());
            Ok(Box::new(KotomemoApp::new(initial_paths, Some(primary_ipc), icon)))
        }),
    )
}
