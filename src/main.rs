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
mod secure_tokens;
mod ui;
mod usecase;

use app::KotomemoApp;
use std::path::PathBuf;
use std::sync::Arc;

fn load_icon() -> egui::IconData {
    let bytes = include_bytes!("../icon-source.png");
    let image = image::load_from_memory(bytes)
        .expect("bundled icon must decode")
        .to_rgba8();
    let (width, height) = image.dimensions();
    egui::IconData {
        rgba: image.into_raw(),
        width,
        height,
    }
}

fn parse_cli_args<I: IntoIterator<Item = String>>(
    args: I,
    cwd: &std::path::Path,
) -> (Vec<PathBuf>, bool) {
    let mut initial_paths = Vec::new();
    let mut memo_mode = false;
    for arg in args {
        if arg == "-memo" || arg == "--memo" {
            memo_mode = true;
            continue;
        }
        let path = PathBuf::from(&arg);
        // `Path::to_absolute()`-equivalent: resolve against CWD without
        // requiring the file to exist yet (a CLI arg naming a
        // not-yet-created file is a valid "prepare this as the save
        // target" case) and without resolving symlinks.
        let resolved = if path.is_absolute() {
            path
        } else {
            cwd.join(path)
        };
        initial_paths.push(resolved);
    }
    (initial_paths, memo_mode)
}

fn main() -> eframe::Result {
    // CLI-arg files (e.g. `kotomemo path/to/file.txt`, or "Open
    // with" registration) plus `-memo` flag:
    let cwd = std::env::current_dir().unwrap_or_default();
    let (initial_paths, memo_mode) = parse_cli_args(std::env::args().skip(1), &cwd);

    let Some(primary_ipc) = ipc::try_acquire_primary_or_forward(&initial_paths, memo_mode) else {
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
            Ok(Box::new(KotomemoApp::new(
                initial_paths,
                memo_mode,
                Some(primary_ipc),
                icon,
            )))
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_cli_args_extracts_memo_flag_and_paths() {
        let cwd = PathBuf::from("/workspace");
        let (paths, memo) = parse_cli_args(vec!["-memo".to_string(), "note.txt".to_string()], &cwd);
        assert!(memo);
        assert_eq!(paths, vec![cwd.join("note.txt")]);

        let (paths_no_memo, memo_off) = parse_cli_args(vec!["a.txt".to_string()], &cwd);
        assert!(!memo_off);
        assert_eq!(paths_no_memo, vec![cwd.join("a.txt")]);
    }
}
