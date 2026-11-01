use crate::editor_state::TabState;
use crate::finder_state::FinderState;
use crate::ui::cursor_position::CursorPosition;
use crate::ui::syntax_colors;
use crate::usecase::bulk_indent::{BulkIndentCommand, BulkIndentInput, IndentMode};
use crate::usecase::highlight;
use egui::text::{CCursorRange, LayoutJob};
use egui::{FontId, TextFormat};

#[derive(Debug, Clone)]
pub struct EditorRenderConfig {
    pub font_id: FontId,
    pub show_line_numbers: bool,
    pub word_wrap: bool,
    pub viewport_height: f32,
    pub viewport_left: f32,
}

pub enum EditorPaneAction {
    None,
    PasteImageRequested,
}

/// Renders one tab's text-editing surface and updates its undo history,
/// cursor position, and (while the Find/Replace bar is open) live
/// selection scope. `id_salt` (the tab's stable `ContentsId`) keeps each
/// tab's scroll offset and widget state separate in egui's own per-id
/// memory store.
pub fn show(
    ui: &mut egui::Ui,
    tab: &mut TabState,
    config: &EditorRenderConfig,
    finder: &mut FinderState,
) -> EditorPaneAction {
    let mut action = EditorPaneAction::None;
    let file_path = tab.contents.file_path.clone();
    let base_color = ui.visuals().text_color();
    let id_salt = tab.contents.id.0.clone();
    let font_id = config.font_id.clone();
    let word_wrap = config.word_wrap;

    let mut layouter = move |ui: &egui::Ui, buf: &dyn egui::TextBuffer, wrap_width: f32| {
        let text = buf.as_str();
        let rule_set = highlight::rules_for(file_path.as_deref());
        let tokens = highlight::highlight(text, rule_set.as_deref());
        let chars: Vec<char> = text.chars().collect();

        let mut job = LayoutJob::default();
        let plain_format = |fid: &FontId| TextFormat {
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
                TextFormat {
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

        job.wrap.max_width = if word_wrap { wrap_width } else { f32::INFINITY };
        ui.fonts_mut(|f| f.layout_job(job))
    };

    let (sel_min, sel_max) = tab.selection;

    let mut outdent = false;
    let mut indent = false;

    if tab.has_focus {
        outdent = ui.input_mut(|i| i.consume_key(egui::Modifiers::SHIFT, egui::Key::Tab));
        indent = !outdent && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Tab));
    }

    if outdent || indent {
        if sel_min != sel_max {
            let mode = if outdent { IndentMode::Outdent } else { IndentMode::Indent };
            let result = BulkIndentCommand::default().execute(BulkIndentInput {
                text: &tab.text,
                selection_start: sel_min,
                selection_end: sel_max,
                mode,
            });
            tab.apply_text(result.text);
            tab.pending_selection = Some((result.selection_start, result.selection_end));
        } else if indent {
            let chars: Vec<char> = tab.text.chars().collect();
            let before: String = chars[..sel_min].iter().collect();
            let after: String = chars[sel_min..].iter().collect();
            let new_text = format!("{before}\t{after}");
            tab.apply_text(new_text);
            let new_cursor = sel_min + 1;
            tab.pending_selection = Some((new_cursor, new_cursor));
        } else {
            let result = BulkIndentCommand::default().execute(BulkIndentInput {
                text: &tab.text,
                selection_start: sel_min,
                selection_end: sel_max,
                mode: IndentMode::Outdent,
            });
            tab.apply_text(result.text);
            tab.pending_selection = Some((result.selection_start, result.selection_end));
        }
    }

    let previous_text = tab.text.clone();
    let previous_selection = tab.selection;

    let row_height = ui
        .fonts_mut(|f| f.row_height(&config.font_id))
        .max(ui.text_style_height(&egui::TextStyle::Monospace))
        .max(1.0);
    // Allow scrolling until the last line of the file reaches the vertical center
    // of the viewport (`viewport_height * 0.5`).
    let half_viewport = (config.viewport_height * 0.5).max(row_height * 4.0);
    let desired_rows = ((config.viewport_height / row_height).floor() as usize).max(1);

    let editor_bg = ui.visuals().extreme_bg_color;
    ui.painter().rect_filled(ui.clip_rect(), 0.0, editor_bg);

    let total_lines = tab.text.bytes().filter(|&b| b == b'\n').count() + 1;
    let digits = total_lines.max(1).ilog10() as usize + 1;
    let digit_width = ui
        .fonts_mut(|f| f.glyph_width(&config.font_id, '0'))
        .max(config.font_id.size * 0.55);
    let gutter_width = if config.show_line_numbers {
        ((digits.max(3) as f32) * digit_width + 18.0).ceil()
    } else {
        0.0
    };

    let mut gutter_rect_opt = None;
    let mut bottom_clicked = false;

    let mut output = ui
        .horizontal_top(|ui| {
            if config.show_line_numbers {
                let (_id, rect) = ui.allocate_space(egui::vec2(gutter_width, row_height));
                gutter_rect_opt = Some(rect);
                ui.add_space(4.0);
            }

            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                let out = egui::TextEdit::multiline(&mut tab.text)
                    .id_salt(&id_salt)
                    .frame(egui::Frame::NONE.fill(editor_bg))
                    .desired_width(f32::INFINITY)
                    .desired_rows(desired_rows)
                    .lock_focus(true)
                    .layouter(&mut layouter)
                    .show(ui);

                // Extra bottom scrollable area so that once text extends past the
                // vertical center of the pane, the last line can always be scrolled
                // up to the vertical middle (`viewport_height * 0.5`).
                let extra_bottom =
                    (out.galley.rect.height() + half_viewport - out.response.rect.height()).max(0.0);
                if extra_bottom > 0.0 {
                    let bottom_space_id = egui::Id::new(("scroll_past_end", &id_salt));
                    let bottom_width = ui.available_width().max(out.response.rect.width()).max(100.0);
                    let (_, bottom_rect) = ui.allocate_space(egui::vec2(bottom_width, extra_bottom));
                    ui.painter().rect_filled(bottom_rect, 0.0, editor_bg);
                    let bottom_resp = ui.interact(bottom_rect, bottom_space_id, egui::Sense::click());
                    if bottom_resp.clicked() {
                        bottom_clicked = true;
                    }
                }
                out
            })
            .inner
        })
        .inner;

    if tab.request_focus {
        tab.request_focus = false;
        output.response.request_focus();
    }

    if bottom_clicked {
        let end_char = tab.text.chars().count();
        tab.pending_selection = Some((end_char, end_char));
    }

    output.response.context_menu(|ui| {
        if ui.button("Paste Image").clicked() {
            action = EditorPaneAction::PasteImageRequested;
            ui.close();
        }
    });

    let jumped = tab.pending_selection.take().inspect(|&(start, end)| {
        let min = egui::text::CCursor::new(start);
        let max = egui::text::CCursor::new(end);
        output.state.cursor.set_char_range(Some(CCursorRange::two(min, max)));
        output.state.store(ui.ctx(), output.response.id);
        output.response.request_focus();
        ui.ctx().request_repaint();
    });

    tab.has_focus = output.response.has_focus();

    let text_changed = tab.text != previous_text;
    tab.note_rendered(previous_text);

    if let Some((start, end)) = jumped {
        tab.cursor_offset = start.max(end);
        tab.selection = (start.min(end), start.max(end));
    } else if let Some(range) = output.cursor_range {
        let primary = range.primary.index.0;
        let secondary = range.secondary.index.0;
        tab.cursor_offset = primary;
        tab.selection = (primary.min(secondary), primary.max(secondary));
    }

    // Paint line numbers aligned with each logical line's visual row in `output.galley`.
    if let Some(gutter_rect) = gutter_rect_opt {
        let clip_rect = ui.clip_rect();
        // Pin the gutter horizontally to `config.viewport_left` so horizontal
        // scrolling (when word wrap is off) does not scroll the line numbers away.
        let gutter_left = gutter_rect.left().max(config.viewport_left);
        let gutter_right = gutter_left + gutter_width;
        let bg_rect = egui::Rect::from_min_max(
            egui::pos2(gutter_left, clip_rect.top()),
            egui::pos2(gutter_right + 2.0, clip_rect.bottom()),
        );
        let painter = ui.painter_at(clip_rect);
        painter.rect_filled(bg_rect, 0.0, editor_bg);
        painter.vline(
            gutter_right,
            clip_rect.y_range(),
            egui::Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color),
        );

        let current_cursor_line = CursorPosition::of(&tab.text, tab.cursor_offset).line;
        let mut logical_line = 1usize;
        for (row_idx, placed_row) in output.galley.rows.iter().enumerate() {
            let is_line_start = if row_idx == 0 {
                true
            } else {
                output.galley.rows[row_idx - 1].ends_with_newline
            };
            if is_line_start {
                let row_y = output.galley_pos.y + placed_row.min_y();
                let row_max_y = output.galley_pos.y + placed_row.max_y();
                if row_max_y >= clip_rect.top() && row_y <= clip_rect.bottom() {
                    let color = if logical_line == current_cursor_line {
                        ui.visuals().strong_text_color()
                    } else {
                        ui.visuals().weak_text_color()
                    };
                    painter.text(
                        egui::pos2(gutter_right - 6.0, row_y),
                        egui::Align2::RIGHT_TOP,
                        logical_line.to_string(),
                        config.font_id.clone(),
                        color,
                    );
                }
                logical_line += 1;
            }
        }
    }

    if finder.visible && jumped.is_none() {
        let selection_changed = tab.selection != previous_selection;
        if !text_changed && selection_changed {
            let (min, max) = tab.selection;
            let is_multiline = min != max && tab.text.chars().skip(min).take(max - min).any(|c| c == '\n');
            finder.scope = if is_multiline { Some((min, max)) } else { None };
        }
    }

    action
}
