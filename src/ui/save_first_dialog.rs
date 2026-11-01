use crate::editor_state::EditorState;
use crate::ui::suggested_filename;

/// Blocking-in-spirit modal shown when the user pastes a clipboard image
/// into an unsaved ("Untitled") tab — kotomemo needs a saved file to know
/// where the shared attachments folder should live. Ports the Kotlin
/// `SaveFirstDialog`.
pub fn show(ctx: &egui::Context, state: &mut EditorState) {
    if state.pending_image_for_attach.is_none() {
        return;
    }
    let mut open = true;

    egui::Window::new("Save this file first")
        .collapsible(false)
        .resizable(false)
        .default_size(egui::vec2(460.0, 160.0))
        .constrain(true)
        .open(&mut open)
        .show(ctx, |ui| {
            ui.label("kotomemo needs to know where to put the attached image.");
            ui.add_space(6.0);
            ui.label(
                egui::RichText::new(
                    "Save this untitled tab to a file, and the image will be attached into the shared \"attachments\" folder next to it.",
                )
                .small()
                .color(ui.visuals().weak_text_color()),
            );
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() {
                    state.cancel_pending_image();
                }
                if ui.button("Save Now…").clicked() {
                    let suggested = state
                        .current()
                        .and_then(|t| {
                            t.contents
                                .file_path
                                .as_ref()
                                .and_then(|p| p.file_name())
                                .map(|n| n.to_string_lossy().into_owned())
                        })
                        .or_else(|| state.current().map(|t| suggested_filename::from(&t.text)))
                        .unwrap_or_else(|| "untitled.txt".to_string());
                    if let Some(path) = rfd::FileDialog::new()
                        .set_title("Save As")
                        .set_file_name(&suggested)
                        .save_file()
                    {
                        let _ = state.save_current(Some(path));
                        state.attach_pending_image();
                    }
                    // Else: the user dismissed the native Save dialog — keep
                    // the pending image so a subsequent "Save Now…" still
                    // works, per the Kotlin original.
                }
            });
        });

    if !open {
        state.cancel_pending_image();
    }
}
