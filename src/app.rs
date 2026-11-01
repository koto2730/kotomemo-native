use crate::editor_state::{EditorState, TabState, TabViewMode};
use crate::entity::LineEnding;
use crate::fonts::{FontConfig, FontSelection};
use crate::ipc::PrimaryInstance;
use crate::ui::api_preset_dialog::{self, DialogAction, DialogState};
use crate::ui::editor_font::FontFamilyKind;
use crate::ui::find_replace_bar::FindReplaceAction;
use crate::ui::images_view::ImagesViewState;
use crate::ui::send_palette::{self, PaletteAction, SendPaletteState};
use crate::ui::tab_bar::TabBarAction;
use crate::ui::{
    editor_pane, find_replace_bar, images_view, save_first_dialog, status_bar, suggested_filename,
    tab_bar,
};
use egui::{FontId, Key, ViewportId};
use std::path::PathBuf;
use std::sync::mpsc::TryRecvError;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DragTarget {
    SameTabBar,
    DetachToNewWindow,
    MergeIntoWindow(ViewportId),
}

#[derive(Debug, Clone)]
struct ActiveTabDrag {
    source_viewport: ViewportId,
    tab_index: usize,
    tab_title: String,
    current_global_pos: egui::Pos2,
}

#[derive(Debug)]
enum WindowCommand {
    None,
    OpenNewBlankWindow,
    DetachTab {
        source_viewport: ViewportId,
        tab_index: usize,
        pos: Option<egui::Pos2>,
    },
    MoveTabToWindow {
        source_viewport: ViewportId,
        tab_index: usize,
        target_viewport: ViewportId,
    },
    CloseWindow(ViewportId),
}

struct EditorWindow {
    viewport_id: ViewportId,
    initial_pos: Option<egui::Pos2>,
    initial_size: egui::Vec2,
    visible: bool,
    focus_requested: bool,
    state: EditorState,
    images: ImagesViewState,
    preset_dialog: Option<DialogState>,
    send_palette: SendPaletteState,
    close_tab_confirm: Option<usize>,
    app_close_confirm_open: bool,
    bypass_close_confirm: bool,
    font_dialog_open: bool,
    last_outer_rect: Option<egui::Rect>,
    last_inner_rect: Option<egui::Rect>,
    last_tab_bar_rect: Option<egui::Rect>,
    last_tab_rects: Vec<egui::Rect>,
}

impl EditorWindow {
    fn new_root(initial_paths: Vec<PathBuf>) -> Self {
        let mut state = EditorState::new();
        for path in &initial_paths {
            let _ = state.open_or_prepare(path);
        }
        state.close_initial_empty_if_possible();
        Self {
            viewport_id: ViewportId::ROOT,
            initial_pos: None,
            initial_size: egui::vec2(900.0, 700.0),
            visible: true,
            focus_requested: false,
            state,
            images: ImagesViewState::default(),
            preset_dialog: None,
            send_palette: SendPaletteState::default(),
            close_tab_confirm: None,
            app_close_confirm_open: false,
            bypass_close_confirm: false,
            font_dialog_open: false,
            last_outer_rect: None,
            last_inner_rect: None,
            last_tab_bar_rect: None,
            last_tab_rects: Vec::new(),
        }
    }

    fn new_secondary(
        viewport_id: ViewportId,
        state: EditorState,
        pos: Option<egui::Pos2>,
        size: egui::Vec2,
    ) -> Self {
        Self {
            viewport_id,
            initial_pos: pos,
            initial_size: size,
            visible: true,
            focus_requested: true,
            state,
            images: ImagesViewState::default(),
            preset_dialog: None,
            send_palette: SendPaletteState::default(),
            close_tab_confirm: None,
            app_close_confirm_open: false,
            bypass_close_confirm: false,
            font_dialog_open: false,
            last_outer_rect: None,
            last_inner_rect: None,
            last_tab_bar_rect: None,
            last_tab_rects: Vec::new(),
        }
    }

    fn window_title(&self) -> String {
        if let Some(tab) = self.state.current() {
            let mark = if tab.contents.is_dirty { " ●" } else { "" };
            format!("{}{mark} - kotomemo", tab.contents.display_name())
        } else {
            "kotomemo".to_string()
        }
    }

    fn summary_label(&self, index_1based: usize) -> String {
        let active_name = self
            .state
            .current()
            .map(|t| t.contents.display_name())
            .unwrap_or_else(|| "Untitled".to_string());
        format!(
            "Window #{index_1based}: {active_name} ({} tab{})",
            self.state.tabs.len(),
            if self.state.tabs.len() == 1 { "" } else { "s" }
        )
    }

    fn handle_dropped_files(&mut self, ui: &egui::Ui) {
        let dropped = ui.input(|i| i.raw.dropped_files.clone());
        for file in dropped {
            let path = file.path();
            if path.is_file() {
                if is_image_file(path) {
                    match image::open(path) {
                        Ok(dynamic_image) => {
                            let rgba_image = dynamic_image.to_rgba8();
                            self.state.paste_clipboard_image(rgba_image);
                        }
                        Err(e) => {
                            self.state.send_status = Some(format!("Failed to open image: {e}"));
                        }
                    }
                } else {
                    let _ = self.state.open_file(path);
                }
            }
        }
    }

    fn open_dialog(&mut self) {
        let files = rfd::FileDialog::new().set_title("Open").pick_files();
        if let Some(paths) = files {
            for path in paths {
                let _ = self.state.open_file(&path);
            }
            self.state.close_initial_empty_if_possible();
        }
    }

    fn save_as_dialog(&mut self) {
        let suggested = self
            .state
            .current()
            .map(|t| suggested_filename::from(&t.text))
            .unwrap_or_else(|| "untitled.txt".to_string());
        if let Some(path) = rfd::FileDialog::new()
            .set_title("Save As")
            .set_file_name(&suggested)
            .save_file()
        {
            let _ = self.state.save_current(Some(path));
        }
    }

    fn save(&mut self) {
        let has_path = self
            .state
            .current()
            .is_some_and(|t| t.contents.file_path.is_some());
        if has_path {
            let _ = self.state.save_current(None);
        } else {
            self.save_as_dialog();
        }
    }

    fn request_close_tab(&mut self, index: usize) {
        if let Some(tab) = self.state.tabs.get(index) {
            if tab.contents.is_dirty {
                self.close_tab_confirm = Some(index);
            } else {
                self.state.close_tab(index);
            }
        }
    }

    fn handle_shortcuts(&mut self, ui: &egui::Ui) -> WindowCommand {
        let mut cmd = WindowCommand::None;
        let mods = egui::Modifiers::COMMAND;
        let mods_shift = egui::Modifiers::COMMAND | egui::Modifiers::SHIFT;
        let (
            equals,
            shift_minus,
            minus,
            num0,
            shift_n,
            n,
            o,
            s,
            w,
            z,
            alt_z,
            y,
            f,
            h,
            one,
            two,
            semicolon,
            q,
            shift_tab,
            tab_key,
        ) = ui.input_mut(|i| {
            (
                i.consume_key(mods, Key::Equals),
                i.consume_key(mods_shift, Key::Minus),
                i.consume_key(mods, Key::Minus),
                i.consume_key(mods, Key::Num0),
                i.consume_key(mods_shift, Key::N),
                i.consume_key(mods, Key::N),
                i.consume_key(mods, Key::O),
                i.consume_key(mods, Key::S),
                i.consume_key(mods, Key::W),
                i.consume_key(mods, Key::Z),
                i.consume_key(egui::Modifiers::ALT, Key::Z),
                i.consume_key(mods, Key::Y),
                i.consume_key(mods, Key::F),
                i.consume_key(mods, Key::H),
                i.consume_key(mods, Key::Num1),
                i.consume_key(mods, Key::Num2),
                i.consume_key(mods, Key::Semicolon),
                i.consume_key(mods, Key::Q),
                i.consume_key(mods_shift, Key::Tab),
                i.consume_key(mods, Key::Tab),
            )
        });

        let escape = self.state.finder.visible
            && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Escape));
        let enter = self.state.finder.visible
            && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Enter));

        if equals || shift_minus {
            self.state.zoom_in();
        }
        if minus {
            self.state.zoom_out();
        }
        if num0 {
            self.state.zoom_reset();
        }
        if shift_n {
            cmd = WindowCommand::OpenNewBlankWindow;
        } else if n {
            self.state.new_tab();
        }
        if o {
            self.open_dialog();
        }
        if s {
            self.save();
        }
        if w {
            self.request_close_tab(self.state.selected_index);
        }
        if z {
            if let Some(tab) = self.state.current_mut() {
                tab.undo();
            }
        }
        if alt_z {
            self.state.toggle_word_wrap();
        }
        if y {
            if let Some(tab) = self.state.current_mut() {
                tab.redo();
            }
        }
        if f {
            let selection = self.state.current().and_then(|t| {
                let (min, max) = t.selection;
                let has_newline =
                    min != max && t.text.chars().skip(min).take(max - min).any(|c| c == '\n');
                if has_newline { Some((min, max)) } else { None }
            });
            self.state.finder.toggle_find(selection);
        }
        if h {
            let selection = self.state.current().and_then(|t| {
                let (min, max) = t.selection;
                let has_newline =
                    min != max && t.text.chars().skip(min).take(max - min).any(|c| c == '\n');
                if has_newline { Some((min, max)) } else { None }
            });
            self.state.finder.toggle_replace(selection);
        }
        if escape {
            self.state.finder.hide();
        }
        if enter {
            use crate::finder_state::FinderMode;
            match self.state.finder.mode {
                FinderMode::Find => self.state.run_find(),
                FinderMode::Replace => self.state.run_replace_one(),
            }
        }
        if one {
            if let Some(tab) = self.state.current_mut() {
                tab.view_mode = TabViewMode::Text;
            }
        }
        if two {
            if let Some(tab) = self.state.current_mut() {
                tab.view_mode = TabViewMode::Images;
            }
        }
        if semicolon {
            let (sel_start, sel_end, cursor_pos) = if let Some(tab) = self.state.current() {
                let id_salt = egui::Id::new(&tab.contents.id.0);
                let range = egui::TextEdit::load_state(ui.ctx(), id_salt)
                    .and_then(|s| s.cursor.char_range());
                match range {
                    Some(r) => {
                        let a = r.primary.index.0;
                        let b = r.secondary.index.0;
                        (a.min(b), a.max(b), a)
                    }
                    None => {
                        let (min, max) = tab.selection;
                        (min, max, tab.cursor_offset)
                    }
                }
            } else {
                (0, 0, 0)
            };
            self.state.send_palette_open = true;
            self.send_palette = SendPaletteState {
                selected_index: 0,
                sel_start,
                sel_end,
                cursor_pos,
            };
        }
        if q {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if shift_tab {
            self.cycle_tab(-1);
        } else if tab_key {
            self.cycle_tab(1);
        }
        cmd
    }

    fn cycle_tab(&mut self, delta: isize) {
        let n = self.state.tabs.len() as isize;
        if n == 0 {
            return;
        }
        let current = self.state.selected_index as isize;
        let next = (((current + delta) % n + n) % n) as usize;
        self.state.selected_index = next;
        if !self.state.is_tiled() {
            self.state.selected_tabs = vec![next];
        }
    }

    fn paste_image_from_clipboard(&mut self) {
        if let Some(image) = crate::clipboard::paste_image() {
            self.state.paste_clipboard_image(image);
        } else {
            self.state.send_status = Some("Clipboard has no image".to_string());
        }
    }

    fn render(
        &mut self,
        ui: &mut egui::Ui,
        other_windows: &[(ViewportId, String)],
        incoming_drop_tab_title: Option<&str>,
        outgoing_drag_hint: Option<&str>,
    ) -> (WindowCommand, Option<(usize, egui::Pos2)>) {
        let mut win_cmd = WindowCommand::None;
        let mut started_drag: Option<(usize, egui::Pos2)> = None;

        // Track window geometry in desktop coordinates
        let (outer_rect, inner_rect, close_requested) = ui.input(|i| {
            let vp = i.viewport();
            (vp.outer_rect, vp.inner_rect, vp.close_requested())
        });
        if outer_rect.is_some() {
            self.last_outer_rect = outer_rect;
        }
        if inner_rect.is_some() {
            self.last_inner_rect = inner_rect;
        }

        if self.focus_requested {
            self.focus_requested = false;
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::Minimized(false));
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Focus);
        }

        ui.ctx()
            .send_viewport_cmd(egui::ViewportCommand::Title(self.window_title()));

        if close_requested {
            if !self.bypass_close_confirm {
                let has_dirty = self.state.tabs.iter().any(|t| t.contents.is_dirty);
                if has_dirty {
                    ui.ctx()
                        .send_viewport_cmd(egui::ViewportCommand::CancelClose);
                    self.app_close_confirm_open = true;
                } else {
                    win_cmd = WindowCommand::CloseWindow(self.viewport_id);
                }
            } else {
                win_cmd = WindowCommand::CloseWindow(self.viewport_id);
            }
        }

        let shortcut_cmd = self.handle_shortcuts(ui);
        if !matches!(shortcut_cmd, WindowCommand::None) {
            win_cmd = shortcut_cmd;
        }
        self.handle_dropped_files(ui);
        self.state.poll_send_result(ui.ctx());

        egui::Panel::top("toolbar").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("New Tab (Ctrl+N)").clicked() {
                        self.state.new_tab();
                        ui.close();
                    }
                    if ui.button("New Window (Ctrl+Shift+N)").clicked() {
                        win_cmd = WindowCommand::OpenNewBlankWindow;
                        ui.close();
                    }
                    if ui.button("Open…").clicked() {
                        self.open_dialog();
                        ui.close();
                    }
                    if ui.button("Save").clicked() {
                        self.save();
                        ui.close();
                    }
                    if ui.button("Save As…").clicked() {
                        self.save_as_dialog();
                        ui.close();
                    }
                    ui.separator();
                    if ui.button("Close Tab").clicked() {
                        self.request_close_tab(self.state.selected_index);
                        ui.close();
                    }
                    ui.separator();
                    if ui.button("Close Window").clicked() {
                        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                        ui.close();
                    }
                });

                ui.menu_button("Edit", |ui| {
                    let can_undo = self.state.current().is_some_and(|t| t.can_undo());
                    let can_redo = self.state.current().is_some_and(|t| t.can_redo());
                    if ui.add_enabled(can_undo, egui::Button::new("Undo")).clicked() {
                        if let Some(tab) = self.state.current_mut() {
                            tab.undo();
                        }
                        ui.close();
                    }
                    if ui.add_enabled(can_redo, egui::Button::new("Redo")).clicked() {
                        if let Some(tab) = self.state.current_mut() {
                            tab.redo();
                        }
                        ui.close();
                    }
                    ui.separator();
                    if ui.button("Select All").clicked() {
                        if let Some(tab) = self.state.current_mut() {
                            let len = tab.text.chars().count();
                            tab.pending_selection = Some((0, len));
                        }
                        ui.close();
                    }
                    ui.separator();
                    if ui.button("Find…").clicked() {
                        let selection = self.state.current().map(|t| t.selection);
                        self.state.finder.toggle_find(selection);
                        ui.close();
                    }
                    if ui.button("Replace…").clicked() {
                        let selection = self.state.current().map(|t| t.selection);
                        self.state.finder.toggle_replace(selection);
                        ui.close();
                    }
                    ui.separator();
                    if ui
                        .button("Paste Image")
                        .on_hover_text("Attach image from clipboard")
                        .clicked()
                    {
                        self.paste_image_from_clipboard();
                        ui.close();
                    }
                });

                ui.menu_button("View", |ui| {
                    if ui
                        .add(egui::Button::selectable(
                            self.state.show_line_numbers,
                            "Show Line Numbers",
                        ))
                        .clicked()
                    {
                        self.state.toggle_line_numbers();
                        ui.close();
                    }
                    if ui
                        .add(egui::Button::selectable(
                            self.state.word_wrap,
                            "Word Wrap (Alt+Z)",
                        ))
                        .clicked()
                    {
                        self.state.toggle_word_wrap();
                        ui.close();
                    }
                    ui.separator();
                    let can_tile_selected = self.state.selected_tabs.len() >= 2;
                    let can_tile_all = self.state.tabs.len() >= 2;
                    if ui
                        .add_enabled(
                            can_tile_selected,
                            egui::Button::new(format!(
                                "Tile Selected Tabs ({})",
                                self.state.selected_tabs.len()
                            )),
                        )
                        .on_hover_text(
                            "Ctrl+Click or Shift+Click tabs in the tab bar to select multiple tabs",
                        )
                        .clicked()
                    {
                        self.state.tile_selected_tabs();
                        ui.close();
                    }
                    if ui
                        .add_enabled(
                            can_tile_all,
                            egui::Button::new(format!("Tile All Tabs ({})", self.state.tabs.len())),
                        )
                        .clicked()
                    {
                        self.state.tile_all_tabs();
                        ui.close();
                    }
                    if ui
                        .add_enabled(
                            self.state.is_tiled(),
                            egui::Button::new("Single Tab View (Untile)"),
                        )
                        .clicked()
                    {
                        self.state.untile_tabs();
                        ui.close();
                    }
                    ui.separator();
                    if ui
                        .add_enabled(
                            self.state.tabs.len() >= 2,
                            egui::Button::new("Move Active Tab to New Window"),
                        )
                        .clicked()
                    {
                        win_cmd = WindowCommand::DetachTab {
                            source_viewport: self.viewport_id,
                            tab_index: self.state.selected_index,
                            pos: None,
                        };
                        ui.close();
                    }
                    if !other_windows.is_empty() {
                        ui.menu_button("Move Active Tab to Window", |ui| {
                            for (target_vp, label) in other_windows {
                                if ui.button(label).clicked() {
                                    win_cmd = WindowCommand::MoveTabToWindow {
                                        source_viewport: self.viewport_id,
                                        tab_index: self.state.selected_index,
                                        target_viewport: *target_vp,
                                    };
                                    ui.close();
                                }
                            }
                        });
                    }
                    ui.separator();
                    if ui.button("Zoom In").clicked() {
                        self.state.zoom_in();
                        ui.close();
                    }
                    if ui.button("Zoom Out").clicked() {
                        self.state.zoom_out();
                        ui.close();
                    }
                    if ui.button("Reset Zoom (100%)").clicked() {
                        self.state.zoom_reset();
                        ui.close();
                    }
                    ui.separator();
                    if ui.button("Font…").clicked() {
                        self.font_dialog_open = true;
                        ui.close();
                    }
                    ui.separator();
                    if let Some(tab) = self.state.current_mut() {
                        if ui.button("Text view").clicked() {
                            tab.view_mode = TabViewMode::Text;
                            ui.close();
                        }
                        if ui.button("Images view").clicked() {
                            tab.view_mode = TabViewMode::Images;
                            ui.close();
                        }
                    }
                });

                ui.menu_button("Format", |ui| {
                    if ui
                        .add(egui::Button::selectable(
                            self.state.word_wrap,
                            "Word Wrap (Alt+Z)",
                        ))
                        .clicked()
                    {
                        self.state.toggle_word_wrap();
                        ui.close();
                    }
                    if ui
                        .add(egui::Button::selectable(
                            self.state.show_line_numbers,
                            "Show Line Numbers",
                        ))
                        .clicked()
                    {
                        self.state.toggle_line_numbers();
                        ui.close();
                    }
                    ui.separator();
                    ui.menu_button("Line Ending", |ui| {
                        if ui.button("LF").clicked() {
                            self.state.set_line_ending(LineEnding::Lf);
                            ui.close();
                        }
                        if ui.button("CRLF").clicked() {
                            self.state.set_line_ending(LineEnding::Crlf);
                            ui.close();
                        }
                    });
                    ui.menu_button("BOM", |ui| {
                        if ui.button("Add").clicked() {
                            self.state.set_bom(true);
                            ui.close();
                        }
                        if ui.button("Remove").clicked() {
                            self.state.set_bom(false);
                            ui.close();
                        }
                    });
                    ui.menu_button("Font", |ui| {
                        if ui.button("Editor Font…").clicked() {
                            self.font_dialog_open = true;
                            ui.close();
                        }
                        ui.separator();
                        ui.menu_button("Select Font", |ui| {
                            for preset in FontSelection::builtin_presets() {
                                let available = preset.is_available();
                                let label = if available {
                                    preset.label()
                                } else {
                                    format!("{} (not installed)", preset.label())
                                };
                                let selected = self.state.font_config.selection == preset;
                                if ui
                                    .add_enabled(
                                        available,
                                        egui::Button::selectable(selected, label),
                                    )
                                    .clicked()
                                {
                                    self.state.font_config.selection = preset;
                                    crate::fonts::apply(ui.ctx(), &self.state.font_config);
                                    let _ = crate::fonts::save_config(&self.state.font_config);
                                    ui.close();
                                }
                            }
                        });
                        ui.menu_button("Family", |ui| {
                            for (label, family) in [
                                ("Monospace", FontFamilyKind::Monospace),
                                ("SansSerif", FontFamilyKind::SansSerif),
                                ("Serif", FontFamilyKind::Serif),
                            ] {
                                if ui.button(label).clicked() {
                                    self.state.set_font_family(family);
                                    ui.close();
                                }
                            }
                        });
                        ui.menu_button("Size", |ui| {
                            for size in [10, 12, 14, 16, 18, 20, 24] {
                                if ui.button(size.to_string()).clicked() {
                                    self.state.set_font_size(size);
                                    ui.close();
                                }
                            }
                        });
                    });
                });

                ui.menu_button("Send", |ui| {
                    let presets = self.state.app_config.presets.clone();
                    let sending = self.state.send_busy;
                    if presets.is_empty() {
                        ui.add_enabled(false, egui::Button::new("(no presets)"));
                    } else {
                        for preset in &presets {
                            let label = if preset.name.trim().is_empty() {
                                "(unnamed)"
                            } else {
                                &preset.name
                            };
                            if ui.add_enabled(!sending, egui::Button::new(label)).clicked() {
                                let (sel_start, sel_end) = if let Some(tab) = self.state.current() {
                                    let id_salt = egui::Id::new(&tab.contents.id.0);
                                    let range = egui::TextEdit::load_state(ui.ctx(), id_salt)
                                        .and_then(|s| s.cursor.char_range());
                                    match range {
                                        Some(r) => {
                                            let a = r.primary.index.0;
                                            let b = r.secondary.index.0;
                                            (a.min(b), a.max(b))
                                        }
                                        None => tab.selection,
                                    }
                                } else {
                                    (0, 0)
                                };
                                self.state
                                    .send_with_preset_at(preset.clone(), sel_start, sel_end);
                                ui.close();
                            }
                        }
                    }
                    ui.separator();
                    if ui.button("Send palette…").on_hover_text("Ctrl+;").clicked() {
                        let (sel_start, sel_end, cursor_pos) =
                            if let Some(tab) = self.state.current() {
                                let id_salt = egui::Id::new(&tab.contents.id.0);
                                let range = egui::TextEdit::load_state(ui.ctx(), id_salt)
                                    .and_then(|s| s.cursor.char_range());
                                match range {
                                    Some(r) => {
                                        let a = r.primary.index.0;
                                        let b = r.secondary.index.0;
                                        (a.min(b), a.max(b), a)
                                    }
                                    None => {
                                        let (min, max) = tab.selection;
                                        (min, max, tab.cursor_offset)
                                    }
                                }
                            } else {
                                (0, 0, 0)
                            };
                        self.state.send_palette_open = true;
                        self.send_palette = SendPaletteState {
                            selected_index: 0,
                            sel_start,
                            sel_end,
                            cursor_pos,
                        };
                        ui.close();
                    }
                    if ui.button("Configure presets…").clicked() {
                        self.preset_dialog =
                            Some(DialogState::from_config(&self.state.app_config));
                        ui.close();
                    }
                });
            });

            if let Some(tab_title) = incoming_drop_tab_title {
                egui::Frame::new()
                    .fill(ui.visuals().selection.bg_fill)
                    .inner_margin(egui::Margin::symmetric(8, 4))
                    .show(ui, |ui| {
                        ui.label(
                            egui::RichText::new(format!(
                                "⬇ Release mouse to move tab \"{tab_title}\" into this window"
                            ))
                            .strong()
                            .color(ui.visuals().selection.stroke.color),
                        );
                    });
            }

            if let Some(hint) = outgoing_drag_hint {
                egui::Frame::new()
                    .fill(ui.visuals().faint_bg_color)
                    .inner_margin(egui::Margin::symmetric(8, 4))
                    .show(ui, |ui| {
                        ui.label(egui::RichText::new(hint).strong());
                    });
            }

            let tab_out = tab_bar::show(ui, &self.state, other_windows);
            self.last_tab_bar_rect = Some(tab_out.bar_rect);
            self.last_tab_rects = tab_out.tab_rects;

            if let Some(tab_idx) = tab_out.drag_started_tab {
                let local_pos = ui
                    .input(|i| i.pointer.latest_pos())
                    .unwrap_or_else(|| tab_out.bar_rect.center());
                let global_pos = self
                    .last_inner_rect
                    .map(|r| r.min + local_pos.to_vec2())
                    .unwrap_or(local_pos);
                started_drag = Some((tab_idx, global_pos));
            }

            match tab_out.action {
                TabBarAction::Select(i) => self.state.select_single_tab(i),
                TabBarAction::ToggleSelect(i) => self.state.toggle_select_tab(i),
                TabBarAction::SelectRange(i) => self.state.select_tab_range(i),
                TabBarAction::Close(i) => self.request_close_tab(i),
                TabBarAction::New => self.state.new_tab(),
                TabBarAction::TileSelected => self.state.tile_selected_tabs(),
                TabBarAction::TileAll => self.state.tile_all_tabs(),
                TabBarAction::Untile => self.state.untile_tabs(),
                TabBarAction::DetachToNewWindow(i) => {
                    win_cmd = WindowCommand::DetachTab {
                        source_viewport: self.viewport_id,
                        tab_index: i,
                        pos: None,
                    };
                }
                TabBarAction::MoveToWindow {
                    tab_index,
                    target_viewport,
                } => {
                    win_cmd = WindowCommand::MoveTabToWindow {
                        source_viewport: self.viewport_id,
                        tab_index,
                        target_viewport,
                    };
                }
                TabBarAction::None => {}
            }

            if self.state.finder.visible {
                match find_replace_bar::show(ui, &mut self.state.finder) {
                    FindReplaceAction::Find => self.state.run_find(),
                    FindReplaceAction::ReplaceOne => self.state.run_replace_one(),
                    FindReplaceAction::ReplaceAll => self.state.run_replace_all(),
                    FindReplaceAction::Close => self.state.finder.hide(),
                    FindReplaceAction::None => {}
                }
            }
        });

        if let Some(tab) = self.state.current() {
            egui::Panel::bottom("status_bar").show(ui, |ui| {
                status_bar::show(
                    ui,
                    tab,
                    self.state.zoom_percent,
                    self.state.send_status.as_deref(),
                );
            });
        }

        save_first_dialog::show(ui.ctx(), &mut self.state);

        if let Some(confirm_idx) = self.close_tab_confirm {
            let mut open = true;
            let tab_name = if let Some(tab) = self.state.tabs.get(confirm_idx) {
                tab.contents.display_name()
            } else {
                "untitled".to_string()
            };

            egui::Window::new("Save changes?")
                .collapsible(false)
                .resizable(false)
                .default_size(egui::vec2(360.0, 120.0))
                .constrain(true)
                .open(&mut open)
                .show(ui.ctx(), |ui| {
                    ui.label(format!("Save changes to \"{}\" before closing?", tab_name));
                    ui.add_space(12.0);
                    ui.horizontal(|ui| {
                        if ui.button("Save").clicked() {
                            let old_index = self.state.selected_index;
                            self.state.selected_index = confirm_idx;
                            self.save();
                            let saved = self
                                .state
                                .tabs
                                .get(confirm_idx)
                                .map(|t| !t.contents.is_dirty)
                                .unwrap_or(false);
                            if saved {
                                self.state.close_tab(confirm_idx);
                                self.close_tab_confirm = None;
                            } else {
                                self.state.selected_index = old_index;
                            }
                        }
                        if ui.button("Don't Save").clicked() {
                            self.state.close_tab(confirm_idx);
                            self.close_tab_confirm = None;
                        }
                        if ui.button("Cancel").clicked() {
                            self.close_tab_confirm = None;
                        }
                    });
                });
            if !open {
                self.close_tab_confirm = None;
            }
        }

        if self.app_close_confirm_open {
            let mut open = true;
            egui::Window::new("Close window?")
                .collapsible(false)
                .resizable(false)
                .default_size(egui::vec2(380.0, 130.0))
                .constrain(true)
                .open(&mut open)
                .show(ui.ctx(), |ui| {
                    ui.label(
                        "You have unsaved changes in this window. Do you want to save them before closing?",
                    );
                    ui.add_space(12.0);
                    ui.horizontal(|ui| {
                        if ui.button("Save & Close").clicked() {
                            let mut all_saved = true;
                            let dirty_indices: Vec<usize> = self
                                .state
                                .tabs
                                .iter()
                                .enumerate()
                                .filter(|(_, t)| t.contents.is_dirty)
                                .map(|(i, _)| i)
                                .collect();
                            for idx in dirty_indices {
                                let old_index = self.state.selected_index;
                                self.state.selected_index = idx;
                                self.save();
                                if self.state.tabs[idx].contents.is_dirty {
                                    all_saved = false;
                                    self.state.selected_index = old_index;
                                    break;
                                }
                            }
                            if all_saved {
                                self.bypass_close_confirm = true;
                                win_cmd = WindowCommand::CloseWindow(self.viewport_id);
                            }
                            self.app_close_confirm_open = false;
                        }
                        if ui.button("Discard Changes & Close").clicked() {
                            self.bypass_close_confirm = true;
                            win_cmd = WindowCommand::CloseWindow(self.viewport_id);
                            self.app_close_confirm_open = false;
                        }
                        if ui.button("Cancel").clicked() {
                            self.app_close_confirm_open = false;
                        }
                    });
                });
            if !open {
                self.app_close_confirm_open = false;
            }
        }

        if let Some(draft) = &mut self.preset_dialog {
            match api_preset_dialog::show(ui.ctx(), draft) {
                DialogAction::Save(config) => {
                    self.state.save_config(config);
                    self.state.send_status = Some(format!(
                        "Settings saved (attachments -> {}/)",
                        self.state.app_config.attachments_folder
                    ));
                    self.preset_dialog = None;
                }
                DialogAction::Cancel => self.preset_dialog = None,
                DialogAction::None => {}
            }
        }

        if self.state.send_palette_open {
            match send_palette::show(
                ui.ctx(),
                &self.state.app_config.presets,
                self.state.last_used_preset_name.as_deref(),
                &mut self.send_palette,
            ) {
                PaletteAction::Send(idx) => {
                    if let Some(preset) = self.state.app_config.presets.get(idx).cloned() {
                        self.state.send_with_preset_at(
                            preset,
                            self.send_palette.sel_start,
                            self.send_palette.sel_end,
                        );
                    }
                    self.state.send_palette_open = false;
                }
                PaletteAction::Close => self.state.send_palette_open = false,
                PaletteAction::None => {}
            }
        }

        if self.font_dialog_open {
            let mut close = false;
            let mut changed = false;
            egui::Window::new("Editor Font")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                .show(ui.ctx(), |ui| {
                    ui.label("Select a font for both ASCII and multi-byte (Japanese) text:");
                    ui.add_space(4.0);
                    for preset in FontSelection::builtin_presets() {
                        let available = preset.is_available();
                        let label = if available {
                            preset.label()
                        } else {
                            format!("{} (not installed)", preset.label())
                        };
                        let selected = self.state.font_config.selection == preset;
                        if ui
                            .add_enabled(available, egui::Button::selectable(selected, label))
                            .clicked()
                        {
                            self.state.font_config.selection = preset;
                            changed = true;
                        }
                    }

                    if let FontSelection::Custom { path, index } =
                        &mut self.state.font_config.selection
                    {
                        let label = FontSelection::Custom {
                            path: path.clone(),
                            index: *index,
                        }
                        .label();
                        let _ = ui.selectable_label(true, label);
                        ui.horizontal(|ui| {
                            ui.label("Collection index (.ttc):");
                            if ui.add(egui::DragValue::new(index).range(0..=16)).changed() {
                                changed = true;
                            }
                        });
                    }

                    if ui
                        .button("Browse Font File (.ttf / .otf / .ttc)…")
                        .clicked()
                    {
                        if let Some(file) = rfd::FileDialog::new()
                            .add_filter(
                                "Font Files",
                                &["ttf", "otf", "ttc", "TTF", "OTF", "TTC"],
                            )
                            .pick_file()
                        {
                            self.state.font_config.selection = FontSelection::Custom {
                                path: file.to_string_lossy().into_owned(),
                                index: 0,
                            };
                            changed = true;
                        }
                    }

                    ui.separator();
                    ui.horizontal(|ui| {
                        ui.label("Editor Font Size:");
                        let mut size = self.state.font_config.clamped_size();
                        if ui
                            .add(
                                egui::Slider::new(&mut size, 10.0..=32.0)
                                    .step_by(0.5)
                                    .suffix(" pt"),
                            )
                            .changed()
                        {
                            self.state.font_config.size = size;
                            self.state.editor_font.size = size.round() as i32;
                            changed = true;
                        }
                        if ui.button("Reset (14 pt)").clicked() {
                            self.state.font_config.size = FontConfig::default().size;
                            self.state.editor_font.size =
                                FontConfig::default().size.round() as i32;
                            changed = true;
                        }
                    });

                    ui.separator();
                    ui.label("Sample Preview:");
                    let preview_font = FontId::new(
                        self.state.font_config.clamped_size(),
                        self.state.editor_font.family.to_egui_family(),
                    );
                    ui.label(
                        egui::RichText::new(
                            "ASCII: 0123456789 ABCDEFG abcdefg !@#$%^&*()_+-=[]{}",
                        )
                        .font(preview_font.clone()),
                    );
                    ui.label(
                        egui::RichText::new(
                            "日本語: ０１２３４５６７８９ ＡＢＣ あいうえお 漢字かな混じり",
                        )
                        .font(preview_font),
                    );

                    ui.separator();
                    if ui.button("Close").clicked() {
                        close = true;
                    }
                });
            if changed {
                crate::fonts::apply(ui.ctx(), &self.state.font_config);
                if let Err(e) = crate::fonts::save_config(&self.state.font_config) {
                    self.state.send_status = Some(format!("Failed to save font settings: {e}"));
                } else {
                    self.state.send_status = Some(format!(
                        "Font updated: {} ({:.1} pt)",
                        self.state.font_config.selection.label(),
                        self.state.font_config.clamped_size()
                    ));
                }
            }
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) || close {
                self.font_dialog_open = false;
            }
        }

        let font_id = FontId::new(
            self.state.effective_font_size(),
            self.state.editor_font.family.to_egui_family(),
        );
        let attachments_folder = self.state.app_config.attachments_folder.clone();
        let show_line_numbers = self.state.show_line_numbers;
        let word_wrap = self.state.word_wrap;

        let mut paste_image_for_tab: Option<usize> = None;
        let mut focus_tab_req: Option<(usize, bool)> = None;
        let mut untile_one_req: Option<usize> = None;
        let mut close_tab_req: Option<usize> = None;

        let central_frame = egui::Frame::central_panel(ui.style())
            .fill(ui.visuals().extreme_bg_color);
        egui::CentralPanel::default().frame(central_frame).show(ui, |ui| {
            let tiled_indices: Vec<usize> = if self.state.is_tiled() {
                self.state
                    .tiled_tabs
                    .iter()
                    .copied()
                    .filter(|&i| i < self.state.tabs.len())
                    .collect()
            } else {
                Vec::new()
            };

            if tiled_indices.len() >= 2 {
                ui.columns(tiled_indices.len(), |columns| {
                    for (col_idx, &tab_idx) in tiled_indices.iter().enumerate() {
                        let col_ui = &mut columns[col_idx];
                        let is_active = tab_idx == self.state.selected_index;

                        col_ui.horizontal(|ui| {
                            let (title, view_mode) = {
                                let tab = &self.state.tabs[tab_idx];
                                let mark = if tab.contents.is_dirty { " ●" } else { "" };
                                (format!("{}{mark}", tab.contents.display_name()), tab.view_mode)
                            };
                            let mut label = egui::RichText::new(title);
                            if is_active {
                                label = label.strong();
                            }
                            if ui
                                .selectable_label(is_active, label)
                                .on_hover_text("Click to focus this pane")
                                .clicked()
                            {
                                focus_tab_req = Some((tab_idx, true));
                            }

                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui.small_button("×").on_hover_text("Close tab").clicked() {
                                        close_tab_req = Some(tab_idx);
                                    }
                                    if ui
                                        .small_button("−")
                                        .on_hover_text(
                                            "Remove from side-by-side view (keep tab open)",
                                        )
                                        .clicked()
                                    {
                                        untile_one_req = Some(tab_idx);
                                    }
                                    let mode_label = match view_mode {
                                        TabViewMode::Text => "Img",
                                        TabViewMode::Images => "Txt",
                                    };
                                    if ui
                                        .small_button(mode_label)
                                        .on_hover_text("Toggle Text / Images view")
                                        .clicked()
                                    {
                                        if let Some(tab) = self.state.tabs.get_mut(tab_idx) {
                                            tab.view_mode = match tab.view_mode {
                                                TabViewMode::Text => TabViewMode::Images,
                                                TabViewMode::Images => TabViewMode::Text,
                                            };
                                        }
                                    }
                                },
                            );
                        });
                        col_ui.separator();

                        let viewport_rect = col_ui.available_rect_before_wrap();
                        let render_config = editor_pane::EditorRenderConfig {
                            font_id: font_id.clone(),
                            show_line_numbers,
                            word_wrap,
                            viewport_height: viewport_rect.height(),
                            viewport_left: viewport_rect.left(),
                        };

                        let view_mode = self.state.tabs[tab_idx].view_mode;
                        let scroll_id = self.state.tabs[tab_idx].contents.id.0.clone();
                        match view_mode {
                            TabViewMode::Text => {
                                let mut action = editor_pane::EditorPaneAction::None;
                                egui::ScrollArea::new([!word_wrap, true])
                                    .id_salt(("editor_scroll_tiled", &scroll_id, col_idx))
                                    .auto_shrink([false, false])
                                    .show(col_ui, |ui| {
                                        let finder = &mut self.state.finder;
                                        if let Some(tab) = self.state.tabs.get_mut(tab_idx) {
                                            action =
                                                editor_pane::show(ui, tab, &render_config, finder);
                                            if tab.has_focus
                                                && self.state.selected_index != tab_idx
                                            {
                                                focus_tab_req = Some((tab_idx, false));
                                            }
                                        }
                                    });
                                if matches!(
                                    action,
                                    editor_pane::EditorPaneAction::PasteImageRequested
                                ) {
                                    paste_image_for_tab = Some(tab_idx);
                                }
                            }
                            TabViewMode::Images => {
                                if let Some(tab) = self.state.tabs.get(tab_idx) {
                                    images_view::show(
                                        col_ui,
                                        tab,
                                        &attachments_folder,
                                        &mut self.images,
                                    );
                                }
                            }
                        }
                    }
                });
            } else {
                let view_mode = self
                    .state
                    .current()
                    .map(|t| t.view_mode)
                    .unwrap_or(TabViewMode::Text);
                let viewport_rect = ui.available_rect_before_wrap();
                let render_config = editor_pane::EditorRenderConfig {
                    font_id,
                    show_line_numbers,
                    word_wrap,
                    viewport_height: viewport_rect.height(),
                    viewport_left: viewport_rect.left(),
                };
                match view_mode {
                    TabViewMode::Text => {
                        let selected_index = self.state.selected_index;
                        let scroll_id = self
                            .state
                            .tabs
                            .get(selected_index)
                            .map(|t| t.contents.id.0.clone())
                            .unwrap_or_default();
                        let mut action = editor_pane::EditorPaneAction::None;
                        egui::ScrollArea::new([!word_wrap, true])
                            .id_salt(("editor_scroll_single", &scroll_id))
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                let finder = &mut self.state.finder;
                                if let Some(tab) = self.state.tabs.get_mut(selected_index) {
                                    action = editor_pane::show(ui, tab, &render_config, finder);
                                }
                            });
                        if matches!(action, editor_pane::EditorPaneAction::PasteImageRequested) {
                            paste_image_for_tab = Some(selected_index);
                        }
                    }
                    TabViewMode::Images => {
                        if let Some(tab) = self.state.current() {
                            images_view::show(ui, tab, &attachments_folder, &mut self.images);
                        }
                    }
                }
            }
        });

        if let Some((tab_idx, req_focus)) = focus_tab_req {
            self.state.selected_index = tab_idx;
            if req_focus {
                if let Some(tab) = self.state.tabs.get_mut(tab_idx) {
                    tab.request_focus = true;
                }
            }
        }
        if let Some(tab_idx) = untile_one_req {
            self.state.remove_from_tile(tab_idx);
        }
        if let Some(tab_idx) = close_tab_req {
            self.request_close_tab(tab_idx);
        }
        if let Some(tab_idx) = paste_image_for_tab {
            self.state.selected_index = tab_idx;
            self.paste_image_from_clipboard();
        }

        (win_cmd, started_drag)
    }
}

pub struct KotomemoApp {
    windows: Vec<EditorWindow>,
    last_focused_viewport: ViewportId,
    next_window_seq: u64,
    icon: Arc<egui::IconData>,
    active_tab_drag: Option<ActiveTabDrag>,
    primary_ipc: Option<PrimaryInstance>,
}

impl KotomemoApp {
    pub fn new(
        initial_paths: Vec<PathBuf>,
        primary_ipc: Option<PrimaryInstance>,
        icon: Arc<egui::IconData>,
    ) -> Self {
        let root_win = EditorWindow::new_root(initial_paths);
        Self {
            windows: vec![root_win],
            last_focused_viewport: ViewportId::ROOT,
            next_window_seq: 1,
            icon,
            active_tab_drag: None,
            primary_ipc,
        }
    }

    fn poll_ipc(&mut self) {
        let Some(ipc) = &self.primary_ipc else { return };
        loop {
            match ipc.rx.try_recv() {
                Ok(paths) => {
                    // Find the most recently active visible window, falling back to
                    // the last visible window.
                    let target_idx = self
                        .windows
                        .iter()
                        .position(|w| w.visible && w.viewport_id == self.last_focused_viewport)
                        .or_else(|| self.windows.iter().rposition(|w| w.visible))
                        .unwrap_or(0);

                    if let Some(win) = self.windows.get_mut(target_idx) {
                        if !win.visible {
                            win.visible = true;
                        }
                        if paths.is_empty() {
                            win.state.new_tab();
                        } else {
                            for path in &paths {
                                let _ = win.state.open_or_prepare(path);
                            }
                            win.state.close_initial_empty_if_possible();
                        }
                        win.focus_requested = true;
                        self.last_focused_viewport = win.viewport_id;
                    }
                }
                Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => break,
            }
        }
    }

    fn classify_drag_target(&self, drag: &ActiveTabDrag) -> DragTarget {
        let pos = drag.current_global_pos;
        let source_win = self
            .windows
            .iter()
            .find(|w| w.visible && w.viewport_id == drag.source_viewport);

        let source_outer = source_win.and_then(|w| w.last_outer_rect);
        let source_tab_bar_global = source_win.and_then(|w| {
            let inner = w.last_inner_rect?;
            let bar = w.last_tab_bar_rect?;
            Some(bar.translate(inner.min.to_vec2()))
        });

        // 1. Check if cursor is over another visible window
        for win in &self.windows {
            if !win.visible || win.viewport_id == drag.source_viewport {
                continue;
            }
            let Some(target_rect) = win.last_outer_rect.or(win.last_inner_rect) else {
                continue;
            };
            if target_rect.contains(pos) {
                let target_top_zone = egui::Rect::from_min_size(
                    target_rect.min,
                    egui::vec2(target_rect.width(), 100.0),
                );
                let outside_source = source_outer.map(|r| !r.contains(pos)).unwrap_or(true);
                if outside_source || target_top_zone.contains(pos) {
                    return DragTarget::MergeIntoWindow(win.viewport_id);
                }
            }
        }

        // 2. Check if cursor was dragged away from the source tab bar to detach into a new window
        if let Some(src) = source_win {
            if src.state.tabs.len() >= 2 {
                if let Some(bar_rect) = source_tab_bar_global {
                    if !bar_rect.expand2(egui::vec2(24.0, 36.0)).contains(pos) {
                        return DragTarget::DetachToNewWindow;
                    }
                }
            }
        }

        DragTarget::SameTabBar
    }

    fn extract_tabs_from_window(
        &mut self,
        source_viewport: ViewportId,
        tab_index: usize,
        keep_at_least_one: bool,
    ) -> Option<(EditorState, Vec<TabState>, egui::Vec2, Option<egui::Pos2>)> {
        let win = self
            .windows
            .iter_mut()
            .find(|w| w.visible && w.viewport_id == source_viewport)?;

        if tab_index >= win.state.tabs.len() {
            return None;
        }
        if keep_at_least_one && win.state.tabs.len() < 2 {
            return None;
        }

        let size = win
            .last_inner_rect
            .map(|r| r.size())
            .unwrap_or(win.initial_size);
        let default_pos = win
            .last_outer_rect
            .map(|r| r.min + egui::vec2(36.0, 36.0));

        // If the dragged tab is part of a multi-selection, move all selected tabs
        // (unless keep_at_least_one is true and all tabs are selected, in which
        // case move just `tab_index`).
        let mut indices_to_move =
            if win.state.selected_tabs.len() > 1 && win.state.selected_tabs.contains(&tab_index) {
                let mut sel = win.state.selected_tabs.clone();
                sel.sort_unstable();
                sel.dedup();
                if keep_at_least_one && sel.len() >= win.state.tabs.len() {
                    vec![tab_index]
                } else {
                    sel
                }
            } else {
                vec![tab_index]
            };

        // Extract in descending index order so earlier indices remain valid, then reverse.
        indices_to_move.sort_unstable_by(|a, b| b.cmp(a));
        let mut extracted = Vec::with_capacity(indices_to_move.len());
        for idx in indices_to_move {
            if let Some(t) = win.state.extract_tab(idx, keep_at_least_one) {
                extracted.push(t);
            }
        }
        extracted.reverse();

        if extracted.is_empty() {
            return None;
        }
        let template = win.state.from_template_with_tabs(Vec::new());
        Some((template, extracted, size, default_pos))
    }

    fn execute_window_command(&mut self, ctx: &egui::Context, cmd: WindowCommand) {
        match cmd {
            WindowCommand::None => {}
            WindowCommand::OpenNewBlankWindow => {
                let template = self
                    .windows
                    .iter()
                    .find(|w| w.visible && w.viewport_id == self.last_focused_viewport)
                    .or_else(|| self.windows.first())
                    .map(|w| {
                        let pos = w.last_outer_rect.map(|r| r.min + egui::vec2(36.0, 36.0));
                        let size = w
                            .last_inner_rect
                            .map(|r| r.size())
                            .unwrap_or(w.initial_size);
                        (
                            w.state
                                .from_template_with_tabs(vec![TabState::new(crate::entity::Contents::empty())]),
                            pos,
                            size,
                        )
                    });
                if let Some((new_state, pos, size)) = template {
                    let seq = self.next_window_seq;
                    self.next_window_seq += 1;
                    let vp_id = ViewportId::from_hash_of(format!("kotomemo_win_{seq}"));
                    self.windows
                        .push(EditorWindow::new_secondary(vp_id, new_state, pos, size));
                    self.last_focused_viewport = vp_id;
                }
            }
            WindowCommand::DetachTab {
                source_viewport,
                tab_index,
                pos,
            } => {
                if let Some((template, extracted, size, default_pos)) =
                    self.extract_tabs_from_window(source_viewport, tab_index, true)
                {
                    let new_state = template.from_template_with_tabs(extracted);
                    let seq = self.next_window_seq;
                    self.next_window_seq += 1;
                    let vp_id = ViewportId::from_hash_of(format!("kotomemo_win_{seq}"));
                    let spawn_pos = pos.or(default_pos);
                    self.windows.push(EditorWindow::new_secondary(
                        vp_id,
                        new_state,
                        spawn_pos,
                        size,
                    ));
                    self.last_focused_viewport = vp_id;
                }
            }
            WindowCommand::MoveTabToWindow {
                source_viewport,
                tab_index,
                target_viewport,
            } => {
                if source_viewport == target_viewport {
                    return;
                }
                if let Some((_template, extracted, _size, _pos)) =
                    self.extract_tabs_from_window(source_viewport, tab_index, false)
                {
                    if let Some(target_win) = self
                        .windows
                        .iter_mut()
                        .find(|w| w.visible && w.viewport_id == target_viewport)
                    {
                        for tab in extracted {
                            target_win.state.insert_tab(tab);
                        }
                        target_win.focus_requested = true;
                        self.last_focused_viewport = target_viewport;
                    }
                    // If the source window now has 0 tabs left, close it!
                    let source_empty = self
                        .windows
                        .iter()
                        .find(|w| w.visible && w.viewport_id == source_viewport)
                        .is_some_and(|w| w.state.tabs.is_empty());
                    if source_empty {
                        self.close_window(ctx, source_viewport);
                    }
                }
            }
            WindowCommand::CloseWindow(vp_id) => {
                self.close_window(ctx, vp_id);
            }
        }
    }

    fn close_window(&mut self, ctx: &egui::Context, vp_id: ViewportId) {
        let visible_count = self.windows.iter().filter(|w| w.visible).count();
        if vp_id == ViewportId::ROOT {
            if visible_count <= 1 {
                if let Some(root) = self.windows.first_mut() {
                    root.bypass_close_confirm = true;
                }
                ctx.send_viewport_cmd_to(ViewportId::ROOT, egui::ViewportCommand::Close);
            } else {
                // Other secondary windows are still open: cancel closing the root
                // process and hide the root viewport instead.
                if let Some(root) = self.windows.first_mut() {
                    root.visible = false;
                    root.state.tabs.clear();
                }
                ctx.send_viewport_cmd_to(ViewportId::ROOT, egui::ViewportCommand::CancelClose);
                ctx.send_viewport_cmd_to(ViewportId::ROOT, egui::ViewportCommand::Visible(false));
                if let Some(next_win) = self.windows.iter().find(|w| w.visible) {
                    self.last_focused_viewport = next_win.viewport_id;
                }
            }
        } else {
            self.windows.retain(|w| w.viewport_id != vp_id);
            let any_visible = self.windows.iter().any(|w| w.visible);
            if !any_visible {
                if let Some(root) = self.windows.first_mut() {
                    root.bypass_close_confirm = true;
                }
                ctx.send_viewport_cmd_to(ViewportId::ROOT, egui::ViewportCommand::Close);
            } else if self.last_focused_viewport == vp_id {
                if let Some(next_win) = self.windows.iter().rfind(|w| w.visible) {
                    self.last_focused_viewport = next_win.viewport_id;
                }
            }
        }
    }
}

impl eframe::App for KotomemoApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.poll_ipc();

        // Snapshot window summaries for "Move Tab to Window" menus
        let visible_summaries: Vec<(ViewportId, String)> = self
            .windows
            .iter()
            .filter(|w| w.visible)
            .enumerate()
            .map(|(idx, w)| (w.viewport_id, w.summary_label(idx + 1)))
            .collect();

        let current_drag_target = self
            .active_tab_drag
            .as_ref()
            .map(|d| self.classify_drag_target(d));

        let mut pending_commands: Vec<WindowCommand> = Vec::new();
        let mut newly_started_drag: Option<ActiveTabDrag> = None;
        let mut any_pointer_down = false;

        // 1. Render Root Window (windows[0])
        if let Some(root_win) = self.windows.first_mut() {
            if root_win.visible {
                if ui.input(|i| i.focused) {
                    self.last_focused_viewport = ViewportId::ROOT;
                }
                if ui.input(|i| i.pointer.primary_down()) {
                    any_pointer_down = true;
                }
                if let Some(drag) = &mut self.active_tab_drag {
                    if drag.source_viewport == ViewportId::ROOT {
                        if let (Some(inner), Some(local)) = (
                            root_win.last_inner_rect,
                            ui.input(|i| i.pointer.latest_pos()),
                        ) {
                            drag.current_global_pos = inner.min + local.to_vec2();
                        }
                    }
                }

                let others: Vec<(ViewportId, String)> = visible_summaries
                    .iter()
                    .filter(|(id, _)| *id != ViewportId::ROOT)
                    .cloned()
                    .collect();

                let incoming_drop = match (&self.active_tab_drag, current_drag_target) {
                    (Some(d), Some(DragTarget::MergeIntoWindow(vp))) if vp == ViewportId::ROOT => {
                        Some(d.tab_title.as_str())
                    }
                    _ => None,
                };
                let outgoing_hint = match (&self.active_tab_drag, current_drag_target) {
                    (Some(d), Some(DragTarget::DetachToNewWindow))
                        if d.source_viewport == ViewportId::ROOT =>
                    {
                        Some("↗ Release outside tab bar to open tab in a New Window")
                    }
                    (Some(d), Some(DragTarget::MergeIntoWindow(_)))
                        if d.source_viewport == ViewportId::ROOT =>
                    {
                        Some("↘ Release to move tab into the target window")
                    }
                    _ => None,
                };

                let (cmd, drag_start) =
                    root_win.render(ui, &others, incoming_drop, outgoing_hint);
                if !matches!(cmd, WindowCommand::None) {
                    pending_commands.push(cmd);
                }
                if let Some((tab_idx, global_pos)) = drag_start {
                    let title = root_win
                        .state
                        .tabs
                        .get(tab_idx)
                        .map(|t| t.contents.display_name())
                        .unwrap_or_default();
                    newly_started_drag = Some(ActiveTabDrag {
                        source_viewport: ViewportId::ROOT,
                        tab_index: tab_idx,
                        tab_title: title,
                        current_global_pos: global_pos,
                    });
                }
            }
        }

        // 2. Render Secondary Windows (windows[1..]) via immediate viewports
        let num_windows = self.windows.len();
        for idx in 1..num_windows {
            if !self.windows[idx].visible {
                continue;
            }
            let vp_id = self.windows[idx].viewport_id;
            let title = self.windows[idx].window_title();
            let initial_size = self.windows[idx].initial_size;
            let initial_pos = self.windows[idx].initial_pos.take();
            let icon = Arc::clone(&self.icon);

            let others: Vec<(ViewportId, String)> = visible_summaries
                .iter()
                .filter(|(id, _)| *id != vp_id)
                .cloned()
                .collect();

            let mut builder = egui::ViewportBuilder::default()
                .with_title(title)
                .with_inner_size(initial_size)
                .with_icon(icon);
            if let Some(pos) = initial_pos {
                builder = builder.with_position(pos);
            }

            let ctx = ui.ctx().clone();
            ctx.show_viewport_immediate(vp_id, builder, |vp_ui, _class| {
                let win = &mut self.windows[idx];
                if vp_ui.input(|i| i.focused) {
                    self.last_focused_viewport = vp_id;
                }
                if vp_ui.input(|i| i.pointer.primary_down()) {
                    any_pointer_down = true;
                }
                if let Some(drag) = &mut self.active_tab_drag {
                    if drag.source_viewport == vp_id {
                        if let (Some(inner), Some(local)) = (
                            win.last_inner_rect,
                            vp_ui.input(|i| i.pointer.latest_pos()),
                        ) {
                            drag.current_global_pos = inner.min + local.to_vec2();
                        }
                    }
                }

                let incoming_drop = match (&self.active_tab_drag, current_drag_target) {
                    (Some(d), Some(DragTarget::MergeIntoWindow(target))) if target == vp_id => {
                        Some(d.tab_title.as_str())
                    }
                    _ => None,
                };
                let outgoing_hint = match (&self.active_tab_drag, current_drag_target) {
                    (Some(d), Some(DragTarget::DetachToNewWindow))
                        if d.source_viewport == vp_id =>
                    {
                        Some("↗ Release outside tab bar to open tab in a New Window")
                    }
                    (Some(d), Some(DragTarget::MergeIntoWindow(_)))
                        if d.source_viewport == vp_id =>
                    {
                        Some("↘ Release to move tab into the target window")
                    }
                    _ => None,
                };

                let (cmd, drag_start) = win.render(vp_ui, &others, incoming_drop, outgoing_hint);
                if !matches!(cmd, WindowCommand::None) {
                    pending_commands.push(cmd);
                }
                if let Some((tab_idx, global_pos)) = drag_start {
                    let tab_title = win
                        .state
                        .tabs
                        .get(tab_idx)
                        .map(|t| t.contents.display_name())
                        .unwrap_or_default();
                    newly_started_drag = Some(ActiveTabDrag {
                        source_viewport: vp_id,
                        tab_index: tab_idx,
                        tab_title,
                        current_global_pos: global_pos,
                    });
                }
            });
        }

        if let Some(new_drag) = newly_started_drag {
            self.active_tab_drag = Some(new_drag);
            any_pointer_down = true;
        }

        // 3. Handle ongoing tab drag (live horizontal reorder or drop on release)
        if let Some(drag) = self.active_tab_drag.clone() {
            ui.ctx().request_repaint();
            let target = self.classify_drag_target(&drag);

            if any_pointer_down {
                // Live horizontal tab reordering while dragging within the source tab bar
                if matches!(target, DragTarget::SameTabBar) {
                    if let Some(win) = self
                        .windows
                        .iter_mut()
                        .find(|w| w.visible && w.viewport_id == drag.source_viewport)
                    {
                        if let Some(inner) = win.last_inner_rect {
                            let local_x = drag.current_global_pos.x - inner.min.x;
                            for (other_idx, rect) in win.last_tab_rects.iter().enumerate() {
                                if other_idx != drag.tab_index
                                    && local_x >= rect.left()
                                    && local_x <= rect.right()
                                {
                                    win.state.move_tab_within(drag.tab_index, other_idx);
                                    if let Some(active) = &mut self.active_tab_drag {
                                        active.tab_index = other_idx;
                                    }
                                    break;
                                }
                            }
                        }
                    }
                }
            } else {
                // Mouse button released: complete the drag operation
                self.active_tab_drag = None;
                match target {
                    DragTarget::SameTabBar => {}
                    DragTarget::DetachToNewWindow => {
                        let spawn_pos = drag.current_global_pos - egui::vec2(80.0, 24.0);
                        pending_commands.push(WindowCommand::DetachTab {
                            source_viewport: drag.source_viewport,
                            tab_index: drag.tab_index,
                            pos: Some(spawn_pos),
                        });
                    }
                    DragTarget::MergeIntoWindow(target_vp) => {
                        pending_commands.push(WindowCommand::MoveTabToWindow {
                            source_viewport: drag.source_viewport,
                            tab_index: drag.tab_index,
                            target_viewport: target_vp,
                        });
                    }
                }
            }
        }

        // 4. Execute any queued window commands
        for cmd in pending_commands {
            self.execute_window_command(ui.ctx(), cmd);
        }
    }
}

fn is_image_file(path: &std::path::Path) -> bool {
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        let ext_lower = ext.to_lowercase();
        matches!(
            ext_lower.as_str(),
            "png" | "jpg" | "jpeg" | "gif" | "bmp" | "webp" | "ico" | "tiff" | "tif"
        )
    } else {
        false
    }
}
