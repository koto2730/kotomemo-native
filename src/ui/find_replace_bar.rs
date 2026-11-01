use crate::finder_state::{FinderMode, FinderState};

pub enum FindReplaceAction {
    None,
    Find,
    ReplaceOne,
    ReplaceAll,
    Close,
}

/// The Find/Replace bar: query field, Find Next / Regex / Aa (case
/// sensitive) / Close, plus (in Replace mode) a replacement field and
/// Replace / Replace All, plus a status line. Ports the Kotlin
/// `FindReplaceBar`. Escape-to-close and Enter-to-run-the-primary-action
/// are both handled globally at the app level, before any widget here
/// renders — see `KotomemoApp::handle_shortcuts` for why Enter in
/// particular isn't handled per-field the way Kotlin's original is.
pub fn show(ui: &mut egui::Ui, finder: &mut FinderState) -> FindReplaceAction {
    let mut action = FindReplaceAction::None;

    ui.horizontal(|ui| {
        let query_resp = ui.text_edit_singleline(&mut finder.query);
        if finder.refocus_query {
            query_resp.request_focus();
        }
        if ui.button("Find Next").clicked() {
            action = FindReplaceAction::Find;
        }
        ui.checkbox(&mut finder.regex, "Regex");
        ui.checkbox(&mut finder.case_sensitive, "Aa");
        if ui.button("Close").clicked() {
            action = FindReplaceAction::Close;
        }
    });

    if finder.mode == FinderMode::Replace {
        ui.horizontal(|ui| {
            ui.text_edit_singleline(&mut finder.replacement);
            if ui.button("Replace").clicked() {
                action = FindReplaceAction::ReplaceOne;
            }
            if ui.button("Replace All").clicked() {
                action = FindReplaceAction::ReplaceAll;
            }
        });
    }

    if let Some(msg) = status_message(finder) {
        ui.label(msg);
    }

    finder.refocus_query = false;
    action
}

fn status_message(finder: &FinderState) -> Option<String> {
    let suffix = if finder.scope.is_some() { " (in selection)" } else { "" };
    if finder.last_replace_count >= 0 {
        let wrapped = if finder.wrapped_around { " — wrapped to top" } else { "" };
        Some(format!("Replaced {}{wrapped}{suffix}", finder.last_replace_count))
    } else if finder.wrapped_around {
        Some(format!("Wrapped to top — Matches: {}{suffix}", finder.last_match_count))
    } else if !finder.query.is_empty() {
        Some(format!("Matches: {}{suffix}", finder.last_match_count))
    } else {
        None
    }
}
