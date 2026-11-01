use crate::entity::ApiPreset;

/// Quick-pick palette for sending the current selection through one of the
/// configured API presets (opened via Ctrl+;). Ports `SendPaletteDialog`.
///
/// Interaction: 1..9 fires presets[0..8] immediately, 0 fires presets[9]
/// (browser-tab numbering — last in the visible group); Up/Down move the
/// highlight; Enter fires the highlighted preset (how presets beyond the
/// first 10 are reachable); Escape closes without sending; click a row to
/// fire it directly.
pub struct SendPaletteState {
    pub selected_index: usize,
    pub sel_start: usize,
    pub sel_end: usize,
    #[allow(dead_code)]
    pub cursor_pos: usize,
}

impl Default for SendPaletteState {
    fn default() -> Self {
        Self {
            selected_index: 0,
            sel_start: 0,
            sel_end: 0,
            cursor_pos: 0,
        }
    }
}

pub enum PaletteAction {
    None,
    Close,
    Send(usize),
}

pub fn show(
    ctx: &egui::Context,
    presets: &[ApiPreset],
    last_used_preset_name: Option<&str>,
    state: &mut SendPaletteState,
) -> PaletteAction {
    let mut action = PaletteAction::None;
    let mut open = true;

    // Find the latest used preset in the current presets list
    let latest_idx = last_used_preset_name
        .and_then(|name| presets.iter().position(|p| p.name == name));
    let has_latest = latest_idx.is_some();
    let total_rows = if has_latest { presets.len() + 1 } else { presets.len() };

    // `consume_key` (not the non-consuming `key_pressed`) so these key
    // presses don't also reach the editor's `TextEdit` underneath, which
    // otherwise keeps keyboard focus while this window is open — an
    // unconsumed Enter would fall through and replace the still-selected
    // text with a newline (unconsumed arrow keys are comparatively benign,
    // just moving/collapsing the editor's own cursor).
    ctx.input_mut(|i| {
        for (key, idx) in [
            (egui::Key::Num1, 0),
            (egui::Key::Num2, 1),
            (egui::Key::Num3, 2),
            (egui::Key::Num4, 3),
            (egui::Key::Num5, 4),
            (egui::Key::Num6, 5),
            (egui::Key::Num7, 6),
            (egui::Key::Num8, 7),
            (egui::Key::Num9, 8),
            (egui::Key::Num0, 9),
        ] {
            if i.consume_key(egui::Modifiers::NONE, key) && idx < presets.len() {
                action = PaletteAction::Send(idx);
            }
        }
        if i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp) && total_rows > 0 {
            state.selected_index = state.selected_index.saturating_sub(1);
        }
        if i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown) && total_rows > 0 {
            state.selected_index = (state.selected_index + 1).min(total_rows - 1);
        }
        if i.consume_key(egui::Modifiers::NONE, egui::Key::Enter) && state.selected_index < total_rows {
            if has_latest {
                if state.selected_index == 0 {
                    action = PaletteAction::Send(latest_idx.unwrap());
                } else {
                    action = PaletteAction::Send(state.selected_index - 1);
                }
            } else {
                action = PaletteAction::Send(state.selected_index);
            }
        }
        if i.consume_key(egui::Modifiers::NONE, egui::Key::Escape) {
            action = PaletteAction::Close;
        }
    });

    egui::Window::new("Send palette")
        .default_size(egui::vec2(560.0, 480.0))
        .collapsible(false)
        .constrain(true)
        .open(&mut open)
        .show(ctx, |ui| {
            if total_rows == 0 {
                ui.centered_and_justified(|ui| {
                    ui.label("No presets configured. Open Send \u{203a} Configure presets\u{2026} to add one.");
                });
                return;
            }
            egui::ScrollArea::vertical().show(ui, |ui| {
                let mut current_row = 0;

                // 1. Draw "Latest" preset entry if available
                if let Some(l_idx) = latest_idx {
                    let preset = &presets[l_idx];
                    let selected = state.selected_index == current_row;
                    let name = if preset.name.trim().is_empty() { "(unnamed)" } else { &preset.name };

                    let response = ui
                        .horizontal(|ui| {
                            ui.set_min_width(ui.available_width());
                            ui.label(egui::RichText::new("★ Latest").strong().color(ui.visuals().hyperlink_color));
                            ui.vertical(|ui| {
                                ui.label(name);
                                ui.label(egui::RichText::new(&preset.url).small().color(ui.visuals().weak_text_color()));
                            });
                        })
                        .response;
                    let row = ui.interact(response.rect, response.id.with(9999), egui::Sense::click());
                    if selected {
                        ui.painter().rect_filled(row.rect, 2.0, ui.visuals().selection.bg_fill.linear_multiply(0.3));
                    }
                    if row.clicked() {
                        action = PaletteAction::Send(l_idx);
                    }
                    ui.separator();
                    current_row += 1;
                }

                // 2. Draw regular presets list
                for (idx, preset) in presets.iter().enumerate() {
                    let number = number_label_for(idx);
                    let name = if preset.name.trim().is_empty() { "(unnamed)" } else { &preset.name };

                    let response = ui
                        .horizontal(|ui| {
                            ui.set_min_width(ui.available_width());
                            ui.label(egui::RichText::new(number).strong());
                            ui.vertical(|ui| {
                                ui.label(name);
                                ui.label(egui::RichText::new(&preset.url).small().color(ui.visuals().weak_text_color()));
                            });
                        })
                        .response;
                    let row = ui.interact(response.rect, response.id.with(idx), egui::Sense::click());
                    let row_selected = state.selected_index == current_row;
                    if row_selected {
                        ui.painter().rect_filled(row.rect, 2.0, ui.visuals().selection.bg_fill.linear_multiply(0.3));
                    }
                    if row.clicked() {
                        action = PaletteAction::Send(idx);
                    }
                    ui.separator();
                    current_row += 1;
                }
            });
        });

    if !open {
        action = PaletteAction::Close;
    }
    action
}

fn number_label_for(idx: usize) -> String {
    match idx {
        0..=8 => (idx + 1).to_string(),
        9 => "0".to_string(),
        _ => " ".to_string(),
    }
}
