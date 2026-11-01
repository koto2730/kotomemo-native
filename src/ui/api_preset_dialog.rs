use crate::entity::{
    ApiPreset, AppConfig, HttpMethod, ResponseTarget, DEFAULT_ATTACHMENTS_FOLDER,
    DEFAULT_REQUEST_TIMEOUT_SECS,
};
use crate::ui::syntax_colors;
use crate::usecase::highlight;
use std::collections::HashMap;

#[derive(Clone)]
pub struct TokenEntry {
    pub key: String,
    pub value: String,
    pub masked: bool,
}

/// Settings dialog: API Presets tab (CRUD list/form for `ApiPreset`s +
/// global tokens) and Editor tab (attachments folder name). Ports the
/// Kotlin `ApiPresetDialog`.
///
/// Draft state (`DialogState`) lives entirely outside `EditorState` and is
/// (re)populated fresh every time the dialog opens — only `Save` commits it
/// back via `EditorState::save_config`; `Cancel` just drops the draft.
pub struct DialogState {
    presets: Vec<ApiPreset>,
    header_drafts: Vec<String>,
    tokens: Vec<TokenEntry>,
    new_token_key: String,
    new_token_value: String,
    attachments_folder_text: String,
    request_timeout_text: String,
    show_line_numbers: bool,
    word_wrap: bool,
    selected_index: Option<usize>,
    active_tab: usize,
}

impl DialogState {
    pub fn from_config(config: &AppConfig) -> Self {
        Self {
            presets: config.presets.clone(),
            header_drafts: config.presets.iter().map(|p| headers_to_text(&p.headers)).collect(),
            tokens: config.tokens.iter().map(|(k, v)| TokenEntry {
                key: k.clone(),
                value: v.clone(),
                masked: true,
            }).collect(),
            new_token_key: String::new(),
            new_token_value: String::new(),
            attachments_folder_text: config.attachments_folder.clone(),
            request_timeout_text: config.request_timeout_secs.to_string(),
            show_line_numbers: config.show_line_numbers,
            word_wrap: config.word_wrap,
            selected_index: if config.presets.is_empty() { None } else { Some(0) },
            active_tab: 0,
        }
    }
}

pub enum DialogAction {
    None,
    Save(AppConfig),
    Cancel,
}

pub fn show(ctx: &egui::Context, draft: &mut DialogState) -> DialogAction {
    let mut action = DialogAction::None;
    let mut open = true;

    egui::Window::new("Settings")
        .default_size(egui::vec2(880.0, 640.0))
        .collapsible(false)
        .resizable(true)
        .constrain(true)
        .scroll([false, true])
        .open(&mut open)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut draft.active_tab, 0, "Presets");
                ui.selectable_value(&mut draft.active_tab, 1, "Tokens");
                ui.selectable_value(&mut draft.active_tab, 2, "Editor");
            });
            ui.separator();

            match draft.active_tab {
                0 => api_presets_tab(ui, draft),
                1 => tokens_tab(ui, draft),
                _ => editor_settings_tab(ui, draft),
            }

            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("Save").clicked() {
                    action = DialogAction::Save(commit(draft));
                }
                if ui.button("Cancel").clicked() {
                    action = DialogAction::Cancel;
                }
            });
        });

    if !open {
        action = DialogAction::Cancel;
    }
    action
}

fn commit(draft: &DialogState) -> AppConfig {
    let tokens = draft.tokens.iter().map(|entry| (entry.key.clone(), entry.value.clone())).collect();
    let final_presets: Vec<ApiPreset> = draft
        .presets
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let mut p = p.clone();
            p.headers = text_to_headers(draft.header_drafts.get(i).map(String::as_str).unwrap_or(""));
            p
        })
        .collect();
    let folder_name = draft.attachments_folder_text.trim();
    let folder_name = if folder_name.is_empty() { DEFAULT_ATTACHMENTS_FOLDER } else { folder_name };
    let request_timeout_secs = draft
        .request_timeout_text
        .trim()
        .parse::<u64>()
        .ok()
        .filter(|secs| *secs > 0)
        .unwrap_or(DEFAULT_REQUEST_TIMEOUT_SECS);
    AppConfig {
        presets: final_presets,
        tokens,
        attachments_folder: folder_name.to_string(),
        request_timeout_secs,
        show_line_numbers: draft.show_line_numbers,
        word_wrap: draft.word_wrap,
    }
}

fn api_presets_tab(ui: &mut egui::Ui, draft: &mut DialogState) {
    ui.horizontal_top(|ui| {
        preset_list(ui, draft);
        ui.separator();
        match draft.selected_index {
            Some(i) if i < draft.presets.len() => preset_form(ui, draft, i),
            _ => {
                ui.centered_and_justified(|ui| ui.label("Select or add a preset"));
            }
        }
    });
}

fn tokens_tab(ui: &mut egui::Ui, draft: &mut DialogState) {
    ui.label("API Tokens (Stored securely)");
    ui.add_space(8.0);

    let mut remove_idx = None;
    for (ti, entry) in draft.tokens.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut entry.key).hint_text("Key (e.g. OPENAI_KEY)").desired_width(180.0));
            if entry.masked {
                let display_masked = if entry.value.is_empty() { "(empty)" } else { "••••••••" };
                ui.label(display_masked);
                if ui.button("✏ Edit").clicked() {
                    entry.masked = false;
                }
            } else {
                ui.add(egui::TextEdit::singleline(&mut entry.value).password(true).desired_width(240.0));
                if ui.button("🔒 Mask").clicked() {
                    entry.masked = true;
                }
            }
            if ui.button("🗑").clicked() {
                remove_idx = Some(ti);
            }
        });
    }
    if let Some(ti) = remove_idx {
        draft.tokens.remove(ti);
    }

    ui.separator();
    ui.horizontal(|ui| {
        ui.add(egui::TextEdit::singleline(&mut draft.new_token_key).hint_text("New Key").desired_width(140.0));
        ui.add(egui::TextEdit::singleline(&mut draft.new_token_value).password(true).hint_text("Token Value / Paste").desired_width(220.0));
        if ui.button("+ Add Token").clicked() {
            if !draft.new_token_key.trim().is_empty() {
                draft.tokens.push(TokenEntry {
                    key: draft.new_token_key.trim().to_string(),
                    value: draft.new_token_value.trim().to_string(),
                    masked: true,
                });
                draft.new_token_key.clear();
                draft.new_token_value.clear();
            }
        }
    });
}

fn preset_list(ui: &mut egui::Ui, draft: &mut DialogState) {
    ui.vertical(|ui| {
        ui.set_width(220.0);
        egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
            if draft.presets.is_empty() {
                ui.label("(empty)");
            }
            for (i, p) in draft.presets.iter().enumerate() {
                let label = if p.name.trim().is_empty() { "(unnamed)".to_string() } else { p.name.clone() };
                let selected = draft.selected_index == Some(i);
                if ui.selectable_label(selected, label).clicked() {
                    draft.selected_index = Some(i);
                }
            }
        });
        ui.horizontal(|ui| {
            if ui.button("Add").clicked() {
                draft.presets.push(ApiPreset {
                    name: format!("new-{}", draft.presets.len() + 1),
                    url: "https://".to_string(),
                    ..Default::default()
                });
                draft.header_drafts.push(String::new());
                draft.selected_index = Some(draft.presets.len() - 1);
            }
            if ui.button("Remove").clicked() {
                if let Some(i) = draft.selected_index {
                    if i < draft.presets.len() {
                        draft.presets.remove(i);
                        draft.header_drafts.remove(i);
                        draft.selected_index = if draft.presets.is_empty() {
                            None
                        } else {
                            Some(i.min(draft.presets.len() - 1))
                        };
                    }
                }
            }
        });
    });
}

fn preset_form(ui: &mut egui::Ui, draft: &mut DialogState, i: usize) {
    egui::ScrollArea::vertical().id_salt("preset_form_scroll").show(ui, |ui| {
        ui.vertical(|ui| {
            ui.set_width(ui.available_width());
            let preset = &mut draft.presets[i];

            ui.label("Name");
            ui.text_edit_singleline(&mut preset.name);

            ui.label("URL — supports {{selection}}, {{tokens.NAME}}, etc.");
            ui.text_edit_singleline(&mut preset.url);

            ui.horizontal(|ui| {
                egui::ComboBox::from_label("Method")
                    .selected_text(format!("{:?}", preset.method))
                    .show_ui(ui, |ui| {
                        for m in [
                            HttpMethod::Get,
                            HttpMethod::Post,
                            HttpMethod::Put,
                            HttpMethod::Patch,
                            HttpMethod::Delete,
                        ] {
                            ui.selectable_value(&mut preset.method, m, format!("{m:?}"));
                        }
                    });
                egui::ComboBox::from_label("Response")
                    .selected_text(format!("{:?}", preset.response_target))
                    .show_ui(ui, |ui| {
                        for t in [ResponseTarget::NewTab, ResponseTarget::AfterSelection, ResponseTarget::StatusOnly] {
                            ui.selectable_value(&mut preset.response_target, t, format!("{t:?}"));
                        }
                    });
            });

            ui.label("Headers (one per line, key: value) — values support {{selection}}, {{tokens.NAME}}, etc.");
            ui.add(
                egui::TextEdit::multiline(&mut draft.header_drafts[i])
                    .desired_rows(4)
                    .hint_text("Authorization: Bearer {{tokens.openai}}\nContent-Type: application/json"),
            );

            ui.label("Body template — use {{selection}}, {{selectionJson}}, {{tokens.NAME}}");
            let mut layouter = |ui: &egui::Ui, buf: &dyn egui::TextBuffer, wrap_width: f32| {
                let text = buf.as_str();
                let rule_set = highlight::rules_for(Some(&std::path::PathBuf::from("dummy.json")));
                let tokens = highlight::highlight(text, rule_set.as_deref());
                let chars: Vec<char> = text.chars().collect();

                let mut job = egui::text::LayoutJob::default();
                let font_id = egui::FontId::monospace(14.0);
                let base_color = ui.visuals().text_color();
                let plain_format = |fid: &egui::FontId| egui::TextFormat {
                    font_id: fid.clone(),
                    color: base_color,
                    ..Default::default()
                };

                let mut pos = 0usize;
                for token in &tokens {
                    if token.start > pos {
                        let plain: String = chars[pos..token.start].iter().collect();
                        job.append(&plain, 0.0, plain_format(&font_id));
                    }
                    let piece: String = chars[token.start..token.end_exclusive].iter().collect();
                    job.append(
                        &piece,
                        0.0,
                        egui::TextFormat {
                            font_id: font_id.clone(),
                            color: syntax_colors::color_of(token.kind),
                            ..Default::default()
                        },
                    );
                    pos = token.end_exclusive;
                }
                if pos < chars.len() {
                    let plain: String = chars[pos..].iter().collect();
                    job.append(&plain, 0.0, plain_format(&font_id));
                }

                job.wrap.max_width = wrap_width;
                ui.fonts_mut(|f| f.layout_job(job))
            };
            ui.add(egui::TextEdit::multiline(&mut preset.body_template).desired_rows(6).layouter(&mut layouter));

            ui.label("Response JSON path (optional, e.g. choices.0.message.content)");
            let mut json_path = preset.response_json_path.clone().unwrap_or_default();
            if ui.text_edit_singleline(&mut json_path).changed() {
                preset.response_json_path = if json_path.trim().is_empty() { None } else { Some(json_path) };
            }
        });
    });
}

fn editor_settings_tab(ui: &mut egui::Ui, draft: &mut DialogState) {
    ui.heading("Attachments");
    ui.label(
        egui::RichText::new(format!(
            "Pasted images are saved into a shared folder next to the edited file. Change the folder name below — the default is \"{DEFAULT_ATTACHMENTS_FOLDER}\". Use \".attachments\" for a hidden folder, or another name like \"img\" if you prefer."
        ))
        .small()
        .color(ui.visuals().weak_text_color()),
    );
    ui.add_space(6.0);
    ui.label("Folder name");
    ui.add(
        egui::TextEdit::singleline(&mut draft.attachments_folder_text)
            .hint_text(DEFAULT_ATTACHMENTS_FOLDER),
    );

    ui.add_space(16.0);
    ui.heading("Network");
    ui.label(
        egui::RichText::new(format!(
            "How long to wait for a Send request before giving up — the default is {DEFAULT_REQUEST_TIMEOUT_SECS} seconds."
        ))
        .small()
        .color(ui.visuals().weak_text_color()),
    );
    ui.add_space(6.0);
    ui.label("Request timeout (seconds)");
    ui.add(
        egui::TextEdit::singleline(&mut draft.request_timeout_text)
            .hint_text(DEFAULT_REQUEST_TIMEOUT_SECS.to_string())
            .desired_width(80.0),
    );
}

fn headers_to_text(headers: &HashMap<String, String>) -> String {
    headers.iter().map(|(k, v)| format!("{k}: {v}")).collect::<Vec<_>>().join("\n")
}

fn text_to_headers(text: &str) -> HashMap<String, String> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#') && line.contains(':'))
        .filter_map(|line| line.split_once(':'))
        .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
        .collect()
}

