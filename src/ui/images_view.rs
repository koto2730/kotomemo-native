use crate::attachments;
use crate::editor_state::TabState;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

const IMAGE_EXTENSIONS: [&str; 5] = ["png", "jpg", "jpeg", "gif", "webp"];
const THUMBNAIL_SIZE: f32 = 150.0;

/// Per-app cache of decoded thumbnails/preview textures, keyed by path —
/// keeps `show` from re-decoding and re-uploading a texture every single
/// frame the gallery is visible. Mirrors the "decoded once per composition
/// lifetime per path" comment on the Kotlin `Thumbnail`/`ImagePreviewModal`
/// (`remember(path)`), just backed by an explicit map instead of Compose's
/// recomposition-scoped memoization.
#[derive(Default)]
pub struct ImagesViewState {
    pub preview_path: Option<PathBuf>,
    cache: HashMap<PathBuf, Option<egui::TextureHandle>>,
}

/// Grid of image thumbnails from the tab's shared attachments folder,
/// newest first. Click a thumbnail to open the full-size preview.
/// Ports the Kotlin `ImagesView`; the `LazyVerticalGrid(Adaptive)` becomes
/// `horizontal_wrapped` (egui has no built-in adaptive-column grid),
/// same simplification already used for the tab strip's overflow handling.
pub fn show(ui: &mut egui::Ui, tab: &TabState, folder_name: &str, state: &mut ImagesViewState) {
    let folder = attachments::folder_for(tab.contents.file_path.as_deref(), folder_name);
    match folder {
        None => empty_placeholder(ui, "Save this file first to attach and view images."),
        Some(folder) if !folder.is_dir() => {
            let name = folder_display_name(&folder);
            empty_placeholder(
                ui,
                &format!("No attachments folder yet ({name}/). Paste an image while in Text view to create one."),
            );
        }
        Some(folder) => {
            let paths = list_images(&folder);
            if paths.is_empty() {
                let name = folder_display_name(&folder);
                empty_placeholder(ui, &format!("No images in {name}/ yet."));
            } else {
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        for path in &paths {
                            thumbnail(ui, path, state);
                        }
                    });
                });
            }
        }
    }

    if let Some(path) = state.preview_path.clone() {
        preview_modal(ui.ctx(), &path, state);
    }
}

fn folder_display_name(folder: &Path) -> String {
    folder.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

fn list_images(folder: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(folder) else { return Vec::new() };
    let mut out: Vec<PathBuf> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .filter(|p| {
            p.extension()
                .and_then(|e| e.to_str())
                .map(|e| IMAGE_EXTENSIONS.contains(&e.to_lowercase().as_str()))
                .unwrap_or(false)
        })
        .collect();
    // Newest first so freshly pasted screenshots appear at the top.
    out.sort_by_key(|p| std::cmp::Reverse(p.metadata().and_then(|m| m.modified()).ok()));
    out
}

fn empty_placeholder(ui: &mut egui::Ui, text: &str) {
    ui.centered_and_justified(|ui| {
        ui.label(egui::RichText::new(text).color(ui.visuals().weak_text_color()));
    });
}

fn load_texture(ctx: &egui::Context, path: &Path) -> Option<egui::TextureHandle> {
    let bytes = std::fs::read(path).ok()?;
    let rgba = image::load_from_memory(&bytes).ok()?.to_rgba8();
    let (w, h) = rgba.dimensions();
    let color_image = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], rgba.as_raw());
    Some(ctx.load_texture(path.to_string_lossy(), color_image, egui::TextureOptions::default()))
}

fn texture_for<'a>(ctx: &egui::Context, path: &Path, cache: &'a mut HashMap<PathBuf, Option<egui::TextureHandle>>) -> &'a Option<egui::TextureHandle> {
    cache.entry(path.to_path_buf()).or_insert_with(|| load_texture(ctx, path))
}

fn thumbnail(ui: &mut egui::Ui, path: &Path, state: &mut ImagesViewState) {
    let ctx = ui.ctx().clone();
    let texture = texture_for(&ctx, path, &mut state.cache).clone();
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();

    ui.allocate_ui(egui::vec2(THUMBNAIL_SIZE, THUMBNAIL_SIZE + 24.0), |ui| {
        ui.vertical(|ui| {
            let clicked = match &texture {
                Some(tex) => ui
                    .add(
                        egui::Image::from_texture(tex)
                            .fit_to_exact_size(egui::vec2(THUMBNAIL_SIZE, THUMBNAIL_SIZE))
                            .sense(egui::Sense::click()),
                    )
                    .clicked(),
                None => {
                    let (rect, response) = ui.allocate_exact_size(
                        egui::vec2(THUMBNAIL_SIZE, THUMBNAIL_SIZE),
                        egui::Sense::click(),
                    );
                    ui.painter().text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        "?",
                        egui::FontId::proportional(28.0),
                        ui.visuals().text_color(),
                    );
                    response.clicked()
                }
            };
            if clicked {
                state.preview_path = Some(path.to_path_buf());
            }
            ui.add(egui::Label::new(egui::RichText::new(name).small()).truncate());
        });
    });
}

fn preview_modal(ctx: &egui::Context, path: &Path, state: &mut ImagesViewState) {
    let texture = texture_for(ctx, path, &mut state.cache).clone();
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let mut open = true;

    egui::Window::new(&name)
        .id(egui::Id::new("image_preview_modal"))
        .default_size(egui::vec2(1000.0, 800.0))
        .constrain(true)
        .open(&mut open)
        .show(ctx, |ui| {
            if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                state.preview_path = None;
            }
            match &texture {
                Some(tex) => {
                    ui.centered_and_justified(|ui| {
                        ui.add(egui::Image::from_texture(tex).shrink_to_fit());
                    });
                }
                None => {
                    ui.centered_and_justified(|ui| {
                        ui.label(format!("Failed to load {name}"));
                    });
                }
            }
        });

    if !open {
        state.preview_path = None;
    }
}
