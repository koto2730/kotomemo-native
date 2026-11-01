use crate::editor_state::TabState;
use crate::entity::{Charset, LineEnding};
use crate::ui::cursor_position::CursorPosition;

/// Bottom status row: cursor position, charset, line ending, BOM flag,
/// zoom %, and an optional transient send/attach-status message.
/// Read-only display, matching the Kotlin `StatusBar`.
pub fn show(ui: &mut egui::Ui, tab: &TabState, zoom_percent: i32, send_status: Option<&str>) {
    ui.horizontal(|ui| {
        let pos = CursorPosition::of(&tab.text, tab.cursor_offset);
        ui.label(format!("Ln {}, Col {}", pos.line, pos.column));
        ui.separator();
        ui.label(charset_label(tab.contents.charset));
        ui.separator();
        ui.label(line_ending_label(tab.contents.line_ending));
        ui.separator();
        ui.label(if tab.contents.has_bom { "BOM" } else { "no BOM" });
        ui.separator();
        ui.label(format!("{zoom_percent}%"));
        if let Some(status) = send_status {
            ui.separator();
            ui.label(status);
        }
    });
}

fn charset_label(charset: Charset) -> &'static str {
    match charset {
        Charset::Utf8 => "UTF-8",
        Charset::Utf16Be => "UTF-16BE",
        Charset::Utf16Le => "UTF-16LE",
    }
}

fn line_ending_label(line_ending: LineEnding) -> &'static str {
    match line_ending {
        LineEnding::Lf => "LF",
        LineEnding::Crlf => "CRLF",
    }
}
