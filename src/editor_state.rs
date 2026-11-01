use crate::attachments;
use crate::entity::{ApiPreset, AppConfig, Contents, ContentsId, ResponseTarget};
use crate::finder_state::FinderState;
use crate::io::{config, file_manager, http_client};
use crate::ui::editor_font::EditorFont;
use crate::usecase::find_replace::{self, FindMatchesInput, ReplaceAllInput};
use crate::usecase::send_request::{self, SendOutput};
use image::RgbaImage;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError};

const MAX_HISTORY: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabViewMode {
    Text,
    Images,
}

/// Per-tab state: the document plus its own undo/redo history. Mirrors the
/// Kotlin `TabState` — a plain snapshot stack of full-buffer text, capped
/// at 200 entries, not an operation/diff-based history.
pub struct TabState {
    pub contents: Contents,
    /// Live buffer bound directly to this tab's `TextEdit` widget.
    pub text: String,
    /// Primary cursor char offset, refreshed from the `TextEdit` output
    /// every frame this tab is rendered.
    pub cursor_offset: usize,
    /// Current selection as (min, max) char offsets (collapsed when equal),
    /// refreshed from the `TextEdit` output every frame this tab is
    /// rendered. Feeds Find/Replace scoping and the "replace this exact
    /// selection" check.
    pub selection: (usize, usize),
    /// A (min, max) char range the editor pane should push into egui's
    /// persisted `TextEditState` on its next render — this is how
    /// Find/Replace "jumps to" a match, since egui's cursor/selection
    /// lives in per-widget memory rather than a field this struct can set
    /// directly (unlike Compose's `TextFieldValue`, which bundles
    /// text+selection in one value the widget just reflects back).
    pub pending_selection: Option<(usize, usize)>,
    pub view_mode: TabViewMode,
    pub has_focus: bool,
    pub request_focus: bool,
    undo_stack: Vec<String>,
    redo_stack: Vec<String>,
}

impl TabState {
    pub fn new(contents: Contents) -> Self {
        Self {
            text: contents.text.clone(),
            contents,
            cursor_offset: 0,
            selection: (0, 0),
            pending_selection: None,
            view_mode: TabViewMode::Text,
            has_focus: false,
            request_focus: false,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
        }
    }

    fn record_text_change(&mut self, previous_text: String) {
        self.undo_stack.push(previous_text);
        if self.undo_stack.len() > MAX_HISTORY {
            self.undo_stack.remove(0);
        }
        self.redo_stack.clear();
        self.contents = self.contents.with_text(self.text.clone());
    }

    /// Called by the editor pane right after rendering this tab's
    /// `TextEdit`, with the text as it was just *before* that render call
    /// (which may have mutated `self.text` in place). Only a real text
    /// change pushes undo history — selection-only changes (cursor moves,
    /// mouse selection) never do, matching Kotlin's `TabState.applyText`.
    pub fn note_rendered(&mut self, previous_text: String) {
        if self.text != previous_text {
            self.record_text_change(previous_text);
        }
    }

    /// Programmatic text change (Find/Replace) — the editor pane's own
    /// before/after diffing (`note_rendered`) never sees this, since the
    /// mutation happens before that frame's render, so it must record its
    /// own undo entry directly. Matches Kotlin's
    /// `tab.applyText(TextFieldValue(newText, ...))` calls from
    /// `runReplaceOne`/`runReplaceAll`.
    pub fn apply_text(&mut self, new_text: String) {
        if new_text != self.text {
            let previous = std::mem::replace(&mut self.text, new_text);
            self.record_text_change(previous);
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    pub fn undo(&mut self) {
        let Some(prev) = self.undo_stack.pop() else { return };
        self.redo_stack.push(std::mem::replace(&mut self.text, prev));
        // Undo always force-dirties, even when it lands back on the exact
        // last-saved text — it does not restore the dirty flag's prior
        // value, matching Kotlin's `contents.copy(text = ..., isDirty = true)`.
        self.contents.text = self.text.clone();
        self.contents.is_dirty = true;
    }

    pub fn redo(&mut self) {
        let Some(next) = self.redo_stack.pop() else { return };
        self.undo_stack.push(std::mem::replace(&mut self.text, next));
        self.contents.text = self.text.clone();
        self.contents.is_dirty = true;
    }

    /// Swaps the underlying `Contents`. `saveCurrent`/`openFile` reuse this;
    /// when the text is unchanged (e.g. a plain Ctrl+S refreshing
    /// `isDirty`/path metadata), the caret and undo/redo history are
    /// preserved instead of resetting the buffer.
    pub fn replace_contents(&mut self, new_contents: Contents) {
        let same_text = new_contents.text == self.text;
        if same_text {
            self.contents = new_contents;
        } else {
            self.text = new_contents.text.clone();
            self.contents = new_contents;
            self.undo_stack.clear();
            self.redo_stack.clear();
            self.cursor_offset = 0;
            self.selection = (0, 0);
            self.pending_selection = None;
        }
    }
}

/// App-wide orchestrator: tab list + selection, shared font/zoom, and
/// Find/Replace state. HTTP-send state lands in M7.
pub struct EditorState {
    pub tabs: Vec<TabState>,
    pub selected_index: usize,
    /// Multi-selected tab indices in the tab bar (via Ctrl+Click or Shift+Click)
    /// used to select N tabs and tile them side-by-side.
    pub selected_tabs: Vec<usize>,
    /// Tab indices currently displayed side-by-side horizontally. When
    /// `tiled_tabs.len() >= 2`, the editor splits into N horizontal columns.
    pub tiled_tabs: Vec<usize>,
    pub show_line_numbers: bool,
    pub word_wrap: bool,
    pub editor_font: EditorFont,
    pub font_config: crate::fonts::FontConfig,
    pub zoom_percent: i32,
    pub finder: FinderState,
    /// Set when the user tried to paste an image into an Untitled tab; the
    /// app shows the "save first" prompt while this is non-`None` so the
    /// user can Save-then-attach in one flow.
    pub pending_image_for_attach: Option<RgbaImage>,
    /// Transient status-line message ("Attached img-....png", a send
    /// result, or a failure).
    pub send_status: Option<String>,
    /// Presets, tokens, and the attachments folder name — loaded from
    /// `~/.kotomemo/config` at startup, written back by `save_config`
    /// (called from the API presets settings dialog).
    pub app_config: AppConfig,
    pub send_busy: bool,
    pub send_palette_open: bool,
    pub last_used_preset_name: Option<String>,
    send_result_rx: Option<Receiver<SendOutcome>>,
}

struct SendOutcome {
    preset: ApiPreset,
    sel_start: usize,
    sel_end: usize,
    output: SendOutput,
}

impl EditorState {
    pub fn new() -> Self {
        let mut app_config = config::load(&config::config_file());
        let original_tokens = app_config.tokens.clone();
        app_config.tokens = crate::secure_tokens::sync_tokens_on_load(original_tokens.clone());

        // If migration happened (plain text tokens existed in the file),
        // save the cleaned configuration immediately to clear plain text from disk.
        if !original_tokens.is_empty() && original_tokens.values().any(|v| !v.is_empty()) {
            let clean_config = AppConfig {
                tokens: crate::secure_tokens::prepare_tokens_for_save(&app_config.tokens),
                ..app_config.clone()
            };
            let _ = config::save(&config::config_file(), &clean_config);
        }

        let font_config = crate::fonts::load_config();
        let mut editor_font = EditorFont::default();
        editor_font.size = font_config.clamped_size().round() as i32;
        let show_line_numbers = app_config.show_line_numbers;
        let word_wrap = app_config.word_wrap;

        let mut state = Self {
            tabs: Vec::new(),
            selected_index: 0,
            selected_tabs: Vec::new(),
            tiled_tabs: Vec::new(),
            show_line_numbers,
            word_wrap,
            editor_font,
            font_config,
            zoom_percent: 100,
            finder: FinderState::default(),
            pending_image_for_attach: None,
            send_status: None,
            app_config,
            send_busy: false,
            send_palette_open: false,
            last_used_preset_name: None,
            send_result_rx: None,
        };
        state.new_tab();
        state
    }

    pub fn save_config(&mut self, new_config: AppConfig) {
        let disk_config = AppConfig {
            tokens: crate::secure_tokens::prepare_tokens_for_save(&new_config.tokens),
            ..new_config.clone()
        };
        let _ = config::save(&config::config_file(), &disk_config);
        self.show_line_numbers = new_config.show_line_numbers;
        self.word_wrap = new_config.word_wrap;
        self.app_config = new_config;
    }

    pub fn toggle_line_numbers(&mut self) {
        self.show_line_numbers = !self.show_line_numbers;
        self.app_config.show_line_numbers = self.show_line_numbers;
        let disk_config = AppConfig {
            tokens: crate::secure_tokens::prepare_tokens_for_save(&self.app_config.tokens),
            ..self.app_config.clone()
        };
        let _ = config::save(&config::config_file(), &disk_config);
    }

    pub fn toggle_word_wrap(&mut self) {
        self.word_wrap = !self.word_wrap;
        self.app_config.word_wrap = self.word_wrap;
        let disk_config = AppConfig {
            tokens: crate::secure_tokens::prepare_tokens_for_save(&self.app_config.tokens),
            ..self.app_config.clone()
        };
        let _ = config::save(&config::config_file(), &disk_config);
    }

    pub fn is_tiled(&self) -> bool {
        self.tiled_tabs.len() >= 2
    }

    pub fn select_single_tab(&mut self, index: usize) {
        if index >= self.tabs.len() {
            return;
        }
        self.selected_index = index;
        if let Some(tab) = self.tabs.get_mut(index) {
            tab.request_focus = true;
        }
        if self.is_tiled() && self.tiled_tabs.contains(&index) {
            // Clicking a tab that is already part of the tiled layout keeps
            // the tiled layout and simply focuses that pane.
            return;
        }
        self.tiled_tabs.clear();
        self.selected_tabs = vec![index];
    }

    pub fn toggle_select_tab(&mut self, index: usize) {
        if index >= self.tabs.len() {
            return;
        }
        if !self.selected_tabs.contains(&self.selected_index) && self.selected_index < self.tabs.len() {
            self.selected_tabs.push(self.selected_index);
        }
        if let Some(pos) = self.selected_tabs.iter().position(|&i| i == index) {
            if self.selected_tabs.len() > 1 {
                self.selected_tabs.remove(pos);
                if self.selected_index == index {
                    self.selected_index = self.selected_tabs[0];
                }
            }
        } else {
            self.selected_tabs.push(index);
            self.selected_index = index;
        }
        if let Some(tab) = self.tabs.get_mut(self.selected_index) {
            tab.request_focus = true;
        }
        if self.is_tiled() {
            if self.selected_tabs.len() >= 2 {
                self.tiled_tabs = self.selected_tabs.clone();
            } else {
                self.tiled_tabs.clear();
            }
        }
    }

    pub fn select_tab_range(&mut self, index: usize) {
        if index >= self.tabs.len() {
            return;
        }
        let start = self.selected_index.min(index);
        let end = self.selected_index.max(index);
        self.selected_tabs = (start..=end).collect();
        self.selected_index = index;
        if let Some(tab) = self.tabs.get_mut(index) {
            tab.request_focus = true;
        }
        if self.is_tiled() && self.selected_tabs.len() >= 2 {
            self.tiled_tabs = self.selected_tabs.clone();
        }
    }

    pub fn tile_selected_tabs(&mut self) {
        let mut valid: Vec<usize> = Vec::new();
        for &idx in &self.selected_tabs {
            if idx < self.tabs.len() && !valid.contains(&idx) {
                valid.push(idx);
            }
        }
        if valid.len() >= 2 {
            if !valid.contains(&self.selected_index) {
                self.selected_index = valid[0];
            }
            self.selected_tabs = valid.clone();
            self.tiled_tabs = valid;
        }
    }

    pub fn tile_all_tabs(&mut self) {
        if self.tabs.len() >= 2 {
            let all: Vec<usize> = (0..self.tabs.len()).collect();
            self.selected_tabs = all.clone();
            self.tiled_tabs = all;
        }
    }

    pub fn untile_tabs(&mut self) {
        self.tiled_tabs.clear();
        if self.selected_index < self.tabs.len() {
            self.selected_tabs = vec![self.selected_index];
        } else {
            self.selected_tabs.clear();
        }
    }

    pub fn remove_from_tile(&mut self, index: usize) {
        self.tiled_tabs.retain(|&i| i != index);
        self.selected_tabs.retain(|&i| i != index);
        if self.tiled_tabs.len() < 2 {
            if let Some(&remaining) = self.tiled_tabs.first() {
                self.selected_index = remaining;
            }
            self.untile_tabs();
        } else if !self.tiled_tabs.contains(&self.selected_index) {
            self.selected_index = self.tiled_tabs[0];
        }
    }

    /// Creates a new `EditorState` for a detached (torn-off) window, copying
    /// visual and app settings from `self` and taking ownership of `tabs`.
    pub fn from_template_with_tabs(&self, mut tabs: Vec<TabState>) -> Self {
        if tabs.is_empty() {
            tabs.push(TabState::new(Contents::empty()));
        }
        for t in &mut tabs {
            t.has_focus = false;
        }
        tabs[0].request_focus = true;
        Self {
            tabs,
            selected_index: 0,
            selected_tabs: vec![0],
            tiled_tabs: Vec::new(),
            show_line_numbers: self.show_line_numbers,
            word_wrap: self.word_wrap,
            editor_font: self.editor_font.clone(),
            font_config: self.font_config.clone(),
            zoom_percent: self.zoom_percent,
            finder: FinderState::default(),
            pending_image_for_attach: None,
            send_status: None,
            app_config: self.app_config.clone(),
            send_busy: false,
            send_palette_open: false,
            last_used_preset_name: self.last_used_preset_name.clone(),
            send_result_rx: None,
        }
    }

    /// Removes and returns `tabs[index]`. When `keep_at_least_one` is true,
    /// a fresh blank tab is created if the last tab was removed; when false
    /// (e.g. merging the last tab of a secondary window into another window),
    /// `self.tabs` is allowed to become empty so the caller can close the window.
    pub fn extract_tab(&mut self, index: usize, keep_at_least_one: bool) -> Option<TabState> {
        if index >= self.tabs.len() {
            return None;
        }
        let removed = self.tabs.remove(index);
        self.selected_tabs.retain(|&i| i != index);
        for idx in &mut self.selected_tabs {
            if *idx > index {
                *idx -= 1;
            }
        }
        self.tiled_tabs.retain(|&i| i != index);
        for idx in &mut self.tiled_tabs {
            if *idx > index {
                *idx -= 1;
            }
        }
        if self.tiled_tabs.len() < 2 {
            self.tiled_tabs.clear();
        }
        if self.tabs.is_empty() {
            if keep_at_least_one {
                self.new_tab();
            } else {
                self.selected_index = 0;
            }
        } else {
            if self.selected_index > index {
                self.selected_index -= 1;
            } else if self.selected_index >= self.tabs.len() {
                self.selected_index = self.tabs.len() - 1;
            }
            if self.is_tiled() && !self.tiled_tabs.contains(&self.selected_index) {
                self.selected_index = self.tiled_tabs[0];
            }
            if self.selected_tabs.is_empty() {
                self.selected_tabs.push(self.selected_index);
            }
        }
        Some(removed)
    }

    /// Appends an existing `TabState` (moved from another window) and selects it.
    pub fn insert_tab(&mut self, mut tab: TabState) {
        tab.has_focus = false;
        tab.request_focus = true;
        self.tabs.push(tab);
        self.selected_index = self.tabs.len() - 1;
        if !self.is_tiled() {
            self.selected_tabs = vec![self.selected_index];
        }
    }

    /// Reorders a tab within the tab bar from `from` to `to`.
    pub fn move_tab_within(&mut self, from: usize, to: usize) {
        if from >= self.tabs.len() || to >= self.tabs.len() || from == to {
            return;
        }
        let remap = |idx: usize| -> usize {
            if idx == from {
                to
            } else if from < to && idx > from && idx <= to {
                idx - 1
            } else if from > to && idx >= to && idx < from {
                idx + 1
            } else {
                idx
            }
        };
        let tab = self.tabs.remove(from);
        self.tabs.insert(to, tab);
        self.selected_index = remap(self.selected_index);
        for idx in &mut self.selected_tabs {
            *idx = remap(*idx);
        }
        for idx in &mut self.tiled_tabs {
            *idx = remap(*idx);
        }
    }

    pub fn effective_font_size(&self) -> f32 {
        ((self.font_config.clamped_size() * self.zoom_percent as f32) / 100.0)
            .max(EditorFont::MIN_SIZE as f32)
    }

    pub fn zoom_in(&mut self) {
        self.zoom_percent = (self.zoom_percent + 10).min(500);
    }

    pub fn zoom_out(&mut self) {
        self.zoom_percent = (self.zoom_percent - 10).max(50);
    }

    pub fn zoom_reset(&mut self) {
        self.zoom_percent = 100;
    }

    pub fn set_line_ending(&mut self, line_ending: crate::entity::LineEnding) {
        if let Some(tab) = self.current_mut() {
            tab.contents.line_ending = line_ending;
            tab.contents.is_dirty = true;
        }
    }

    pub fn set_bom(&mut self, with_bom: bool) {
        if let Some(tab) = self.current_mut() {
            tab.contents.has_bom = with_bom;
            tab.contents.is_dirty = true;
        }
    }

    pub fn set_font_family(&mut self, family: crate::ui::editor_font::FontFamilyKind) {
        self.editor_font.family = family;
    }

    pub fn set_font_size(&mut self, size: i32) {
        let clamped = size.clamp(EditorFont::MIN_SIZE, EditorFont::MAX_SIZE);
        self.editor_font.size = clamped;
        self.font_config.size = clamped as f32;
        let _ = crate::fonts::save_config(&self.font_config);
    }

    pub fn current(&self) -> Option<&TabState> {
        self.tabs.get(self.selected_index)
    }

    pub fn current_mut(&mut self) -> Option<&mut TabState> {
        self.tabs.get_mut(self.selected_index)
    }

    pub fn new_tab(&mut self) {
        self.tabs.push(TabState::new(Contents::empty()));
        self.selected_index = self.tabs.len() - 1;
        if !self.is_tiled() {
            self.selected_tabs = vec![self.selected_index];
        }
    }

    pub fn open_file(&mut self, path: &Path) -> std::io::Result<()> {
        let loaded = file_manager::load(path)?;
        let contents = Contents {
            id: ContentsId::generate(),
            text: loaded.text,
            file_path: Some(path.to_path_buf()),
            charset: loaded.charset,
            line_ending: loaded.line_ending,
            has_bom: loaded.has_bom,
            is_dirty: false,
        };
        // Re-opening an already-open file reuses that tab rather than
        // creating a duplicate.
        if let Some(existing) = self
            .tabs
            .iter()
            .position(|t| t.contents.file_path.as_deref() == Some(path))
        {
            self.tabs[existing].replace_contents(contents);
            self.selected_index = existing;
        } else {
            self.tabs.push(TabState::new(contents));
            self.selected_index = self.tabs.len() - 1;
        }
        if !self.is_tiled() {
            self.selected_tabs = vec![self.selected_index];
        }
        Ok(())
    }

    /// Used for CLI-arg paths: opens the file if it exists, otherwise
    /// prepares an empty buffer already targeting that path so a
    /// subsequent Save writes there directly.
    pub fn open_or_prepare(&mut self, path: &Path) -> std::io::Result<()> {
        if path.exists() {
            self.open_file(path)
        } else {
            let mut empty = Contents::empty();
            empty.file_path = Some(path.to_path_buf());
            self.tabs.push(TabState::new(empty));
            self.selected_index = self.tabs.len() - 1;
            if !self.is_tiled() {
                self.selected_tabs = vec![self.selected_index];
            }
            Ok(())
        }
    }

    /// Drops the initial blank tab once real tabs have been opened
    /// alongside it (CLI-arg startup), the same way the Kotlin app does.
    pub fn close_initial_empty_if_possible(&mut self) {
        if self.tabs.len() > 1
            && self.tabs[0].contents.file_path.is_none()
            && self.tabs[0].contents.text.is_empty()
        {
            self.close_tab(0);
        }
    }

    pub fn save_current(&mut self, target_path: Option<PathBuf>) -> std::io::Result<()> {
        let Some(index) = self.tabs.get(self.selected_index).map(|_| self.selected_index) else {
            return Ok(());
        };
        let tab = &self.tabs[index];
        let Some(path) = target_path.or_else(|| tab.contents.file_path.clone()) else {
            return Ok(());
        };
        file_manager::save(
            &path,
            &tab.text,
            tab.contents.charset,
            tab.contents.line_ending,
            tab.contents.has_bom,
        )?;
        let mut saved = tab.contents.clone();
        saved.text = tab.text.clone();
        let saved = saved.mark_saved(path);
        self.tabs[index].replace_contents(saved);
        Ok(())
    }

    /// Closing the last tab always immediately opens a fresh blank one —
    /// the app can never reach zero tabs. No unsaved-changes confirmation,
    /// matching the Kotlin app's current (deliberate) behaviour.
    pub fn close_tab(&mut self, index: usize) {
        if index >= self.tabs.len() {
            return;
        }
        self.tabs.remove(index);
        self.selected_tabs.retain(|&i| i != index);
        for idx in &mut self.selected_tabs {
            if *idx > index {
                *idx -= 1;
            }
        }
        self.tiled_tabs.retain(|&i| i != index);
        for idx in &mut self.tiled_tabs {
            if *idx > index {
                *idx -= 1;
            }
        }
        if self.tiled_tabs.len() < 2 {
            self.tiled_tabs.clear();
        }
        if self.tabs.is_empty() {
            self.new_tab();
        } else {
            if self.selected_index > index {
                self.selected_index -= 1;
            } else if self.selected_index >= self.tabs.len() {
                self.selected_index = self.tabs.len() - 1;
            }
            if self.is_tiled() && !self.tiled_tabs.contains(&self.selected_index) {
                self.selected_index = self.tiled_tabs[0];
            }
            if self.selected_tabs.is_empty() {
                self.selected_tabs.push(self.selected_index);
            }
        }
    }

    /// Forward-only search with wraparound: finds the first match at or
    /// after the cursor within `finder.scope` (or the whole document);
    /// wraps to the first match in range if none exists after the cursor.
    /// Ports `EditorState.runFind` exactly.
    pub fn run_find(&mut self) {
        let index = self.selected_index;
        let Some(tab) = self.tabs.get(index) else { return };
        let chars: Vec<char> = tab.text.chars().collect();
        let len = chars.len();
        let (search_from, search_to) = match self.finder.scope {
            Some((s, e)) => (s.min(len), e.min(len)),
            None => (0, len),
        };
        let substring: String = chars[search_from..search_to].iter().collect();
        let cursor = tab.selection.1;

        let raw_matches = find_replace::find_matches(&FindMatchesInput {
            text: &substring,
            query: &self.finder.query,
            regex: self.finder.regex,
            case_sensitive: self.finder.case_sensitive,
        });
        // Offset matches (found within the scoped substring) back into
        // full-document coordinates — the jump they drive has to land in
        // the real document, not the substring.
        let matches: Vec<(usize, usize)> = raw_matches
            .into_iter()
            .map(|(s, e)| (s + search_from, e + search_from))
            .collect();

        self.finder.last_match_count = matches.len();
        self.finder.last_replace_count = -1;
        if matches.is_empty() {
            self.finder.wrapped_around = false;
            return;
        }

        // A cursor outside the scope (e.g. right after opening Find with a
        // fresh selection) shouldn't suppress every match; treat it as
        // "start of scope" so the first match in range is still found.
        let effective_cursor = if cursor < search_from || cursor > search_to {
            search_from
        } else {
            cursor
        };
        let next_after = matches.iter().find(|(start, _)| *start >= effective_cursor).copied();
        let target = next_after.unwrap_or(matches[0]);
        self.finder.wrapped_around = next_after.is_none();

        if let Some(tab) = self.tabs.get_mut(index) {
            tab.pending_selection = Some((target.0, target.1 + 1));
        }
    }

    /// Replaces only when the current selection is *exactly* one whole
    /// match for the query under the current regex/case settings —
    /// otherwise silently degrades to "Find Next" instead. After
    /// replacing, auto-advances to the next occurrence. Ports
    /// `EditorState.runReplaceOne` exactly.
    pub fn run_replace_one(&mut self) {
        let index = self.selected_index;
        let Some(tab) = self.tabs.get(index) else { return };
        let (sel_min, sel_max) = tab.selection;
        if sel_min == sel_max {
            self.run_find();
            return;
        }

        let chars: Vec<char> = tab.text.chars().collect();
        let selected_text: String = chars[sel_min..sel_max].iter().collect();
        let sel_char_len = selected_text.chars().count();

        let matches_in_selection = find_replace::find_matches(&FindMatchesInput {
            text: &selected_text,
            query: &self.finder.query,
            regex: self.finder.regex,
            case_sensitive: self.finder.case_sensitive,
        });
        let is_full_match = matches_in_selection.len() == 1
            && matches_in_selection[0].0 == 0
            && matches_in_selection[0].1 + 1 == sel_char_len;
        if !is_full_match {
            self.run_find();
            return;
        }

        let r = find_replace::replace_all(&ReplaceAllInput {
            text: &selected_text,
            query: &self.finder.query,
            replacement: &self.finder.replacement,
            regex: self.finder.regex,
            case_sensitive: self.finder.case_sensitive,
        });
        let before: String = chars[..sel_min].iter().collect();
        let after: String = chars[sel_max..].iter().collect();
        let new_text = format!("{before}{}{after}", r.text);
        let cursor = sel_min + r.text.chars().count();

        if let Some(tab) = self.tabs.get_mut(index) {
            tab.apply_text(new_text);
            tab.pending_selection = Some((cursor, cursor));
        }
        // `run_find` resets `last_replace_count` to -1, so it must be set
        // *after* the call, same ordering the Kotlin original relies on.
        self.run_find();
        self.finder.last_replace_count = 1;
    }

    /// Unscoped: replaces across the whole document, cursor collapses to
    /// the start. Scoped: replaces only within `finder.scope`, splices the
    /// result back in, and resizes the scope to track the (possibly
    /// grown/shrunk) replaced region, leaving it selected. A zero-match
    /// Replace All makes no state change at all (no undo entry). Ports
    /// `EditorState.runReplaceAll` exactly.
    pub fn run_replace_all(&mut self) {
        let index = self.selected_index;
        let Some(tab) = self.tabs.get(index) else { return };
        let full_text = tab.text.clone();
        let chars: Vec<char> = full_text.chars().collect();
        let len = chars.len();

        match self.finder.scope {
            None => {
                let r = find_replace::replace_all(&ReplaceAllInput {
                    text: &full_text,
                    query: &self.finder.query,
                    replacement: &self.finder.replacement,
                    regex: self.finder.regex,
                    case_sensitive: self.finder.case_sensitive,
                });
                self.finder.last_replace_count = r.count as i64;
                if r.count > 0 {
                    if let Some(tab) = self.tabs.get_mut(index) {
                        tab.apply_text(r.text);
                        tab.pending_selection = Some((0, 0));
                    }
                }
            }
            Some((from, to)) => {
                let from = from.min(len);
                let to = to.min(len);
                let substring: String = chars[from..to].iter().collect();
                let r = find_replace::replace_all(&ReplaceAllInput {
                    text: &substring,
                    query: &self.finder.query,
                    replacement: &self.finder.replacement,
                    regex: self.finder.regex,
                    case_sensitive: self.finder.case_sensitive,
                });
                self.finder.last_replace_count = r.count as i64;
                if r.count > 0 {
                    let before: String = chars[..from].iter().collect();
                    let after: String = chars[to..].iter().collect();
                    let new_text = format!("{before}{}{after}", r.text);
                    let new_scope_end = from + r.text.chars().count();
                    self.finder.scope = Some((from, new_scope_end));
                    if let Some(tab) = self.tabs.get_mut(index) {
                        tab.apply_text(new_text);
                        tab.pending_selection = Some((from, new_scope_end));
                    }
                }
            }
        }
    }

    /// Entry point for a clipboard image paste (checked ahead of the
    /// default text paste — see `KotomemoApp`'s Ctrl+V handling). If the
    /// current tab has no on-disk path yet, the image is cached and the
    /// "save first" prompt takes over; otherwise it's attached right away.
    /// Ports `EditorState.pasteImage` (the private overload).
    pub fn paste_clipboard_image(&mut self, image: RgbaImage) {
        let Some(tab) = self.current() else { return };
        match attachments::folder_for(tab.contents.file_path.as_deref(), &self.app_config.attachments_folder) {
            None => self.pending_image_for_attach = Some(image),
            Some(folder) => self.save_pasted_image(folder, image),
        }
    }

    /// Called after `SaveFirstDialog`'s "Save Now…" successfully saves the
    /// tab. Ports `EditorState.attachPendingImage`.
    pub fn attach_pending_image(&mut self) {
        let Some(image) = self.pending_image_for_attach.take() else { return };
        let Some(tab) = self.current() else { return };
        let Some(folder) = attachments::folder_for(tab.contents.file_path.as_deref(), &self.app_config.attachments_folder) else {
            return;
        };
        self.save_pasted_image(folder, image);
    }

    pub fn cancel_pending_image(&mut self) {
        self.pending_image_for_attach = None;
    }

    /// Timestamp-to-the-second filename, so two images pasted within the
    /// same second collide and overwrite each other — an existing Kotlin
    /// behaviour quirk, not something to fix here.
    fn save_pasted_image(&mut self, folder: PathBuf, image: RgbaImage) {
        let index = self.selected_index;
        let result = (|| -> Result<String, String> {
            std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
            let now = time::OffsetDateTime::now_utc();
            let filename = format!(
                "img-{:04}{:02}{:02}-{:02}{:02}{:02}.png",
                now.year(),
                u8::from(now.month()),
                now.day(),
                now.hour(),
                now.minute(),
                now.second()
            );
            image.save(folder.join(&filename)).map_err(|e| e.to_string())?;
            Ok(filename)
        })();

        match result {
            Ok(filename) => {
                let reference = format!("[img: {filename}]");
                if let Some(tab) = self.tabs.get_mut(index) {
                    let (sel_min, sel_max) = tab.selection;
                    let chars: Vec<char> = tab.text.chars().collect();
                    let before: String = chars[..sel_min].iter().collect();
                    let after: String = chars[sel_max..].iter().collect();
                    let new_text = format!("{before}{reference}{after}");
                    let cursor = sel_min + reference.chars().count();
                    tab.apply_text(new_text);
                    tab.pending_selection = Some((cursor, cursor));
                }
                self.send_status = Some(format!("Attached {filename}"));
            }
            Err(e) => {
                self.send_status = Some(format!("Failed to attach image: {e}"));
            }
        }
    }

    /// Fires `preset` against the current selection (whole document if
    /// nothing is selected) on a background thread, so the HTTP round-trip
    /// never blocks a redraw. `poll_send_result` must be called every
    /// frame to notice when it lands (no automatic cross-thread state
    /// observation in an immediate-mode UI — egui just re-runs `ui()` on
    /// its own schedule, it doesn't watch arbitrary background state).
    /// Ports `EditorState.sendWithPreset`.


    pub fn send_with_preset_at(&mut self, preset: ApiPreset, sel_start: usize, sel_end: usize) {
        if self.send_busy {
            return;
        }
        let Some(tab) = self.current() else { return };
        let selected_text = if sel_start == sel_end {
            tab.text.clone()
        } else {
            tab.text.chars().skip(sel_start).take(sel_end - sel_start).collect()
        };
        let filename = tab.contents.display_name();
        let tokens = self.app_config.tokens.clone();
        let timeout = std::time::Duration::from_secs(self.app_config.request_timeout_secs);

        self.last_used_preset_name = Some(preset.name.clone());
        self.send_busy = true;
        self.send_status = Some(format!("Sending to '{}'…", preset.name));

        let (tx, rx) = std::sync::mpsc::channel();
        let preset_for_thread = preset.clone();
        std::thread::Builder::new()
            .name("kotomemo-send".to_string())
            .spawn(move || {
                let client = http_client::UreqHttpClient::new(timeout);
                let output = send_request::send_request(
                    &client,
                    send_request::SendRequestInput {
                        preset: &preset_for_thread,
                        selection: &selected_text,
                        filename: &filename,
                        tokens: &tokens,
                    },
                );
                let _ = tx.send(SendOutcome {
                    preset: preset_for_thread,
                    sel_start,
                    sel_end,
                    output,
                });
            })
            .expect("failed to spawn kotomemo-send thread");
        self.send_result_rx = Some(rx);
    }

    /// Call once per frame. Requests another repaint soon while still
    /// waiting, so the UI notices the result promptly instead of only on
    /// the next input-driven redraw.
    pub fn poll_send_result(&mut self, ctx: &egui::Context) {
        let Some(rx) = &self.send_result_rx else { return };
        match rx.try_recv() {
            Ok(outcome) => {
                self.send_result_rx = None;
                self.handle_send_result(outcome);
            }
            Err(TryRecvError::Empty) => {
                ctx.request_repaint();
            }
            Err(TryRecvError::Disconnected) => {
                self.send_result_rx = None;
                self.send_busy = false;
                self.send_status = Some("Send failed unexpectedly".to_string());
            }
        }
    }

    fn handle_send_result(&mut self, outcome: SendOutcome) {
        match outcome.output {
            SendOutput::Failure { message, raw_body, .. } => {
                // The status bar is the only place a failed send shows up,
                // so fold in a snippet of the response body — "HTTP 400"
                // alone gives no hint that e.g. the body template produced
                // invalid JSON, which the server's error message usually
                // says outright.
                let snippet = raw_body.as_deref().map(collapse_whitespace).filter(|s| !s.is_empty());
                self.send_status = Some(match snippet {
                    Some(s) => format!("Send failed: {message} - {s}"),
                    None => format!("Send failed: {message}"),
                });
            }
            SendOutput::Success { extracted, status, .. } => {
                self.send_status = Some(format!("Send OK ({status})"));
                match outcome.preset.response_target {
                    ResponseTarget::NewTab => {
                        let mut contents = Contents::empty();
                        contents.text = extracted;
                        contents.is_dirty = true;
                        self.tabs.push(TabState::new(contents));
                        self.selected_index = self.tabs.len() - 1;
                        if !self.is_tiled() {
                            self.selected_tabs = vec![self.selected_index];
                        }
                    }
                    ResponseTarget::AfterSelection => {
                        self.insert_after_selection(outcome.sel_start, outcome.sel_end, &extracted);
                    }
                    ResponseTarget::StatusOnly => {}
                }
            }
        }
        self.send_busy = false;
    }

    fn insert_after_selection(&mut self, sel_start: usize, sel_end: usize, response: &str) {
        let index = self.selected_index;
        let Some(tab) = self.tabs.get(index) else { return };
        // HTTP servers default to CRLF. Normalise to LF so the buffer
        // invariant (logical LF only) holds.
        let normalized = response.replace("\r\n", "\n").replace('\r', "\n");
        let chars: Vec<char> = tab.text.chars().collect();
        let end = sel_end.min(chars.len());
        // Land at the start of the next line: insert a newline before if
        // needed, and after if the response doesn't already end in one.
        let needs_newline_before = end > 0 && chars.get(end - 1) != Some(&'\n');
        let needs_newline_after = !normalized.ends_with('\n');
        let mut payload = String::new();
        if needs_newline_before {
            payload.push('\n');
        }
        payload.push_str(&normalized);
        if needs_newline_after {
            payload.push('\n');
        }
        let before: String = chars[..end].iter().collect();
        let after: String = chars[end..].iter().collect();
        let new_text = format!("{before}{payload}{after}");

        if let Some(tab) = self.tabs.get_mut(index) {
            tab.apply_text(new_text);
            // Restore the original selection — offsets to its left are
            // unaffected since the insertion point is at/after `sel_end`.
            tab.pending_selection = Some((sel_start, sel_end));
        }
    }
}

fn collapse_whitespace(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ").trim().chars().take(160).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multi_select_and_tile_three_tabs() {
        let mut state = EditorState::new();
        state.new_tab();
        state.new_tab();
        state.new_tab();
        assert_eq!(state.tabs.len(), 4);

        state.select_single_tab(0);
        assert_eq!(state.selected_tabs, vec![0]);
        assert!(!state.is_tiled());

        state.toggle_select_tab(1);
        state.toggle_select_tab(3);
        assert_eq!(state.selected_tabs, vec![0, 1, 3]);
        assert!(!state.is_tiled());

        state.tile_selected_tabs();
        assert!(state.is_tiled());
        assert_eq!(state.tiled_tabs, vec![0, 1, 3]);

        // Closing tab 1 shifts tab 3 -> 2 and keeps the remaining two tabs tiled
        state.close_tab(1);
        assert!(state.is_tiled());
        assert_eq!(state.tiled_tabs, vec![0, 2]);

        // Removing one more tab from tile drops below 2 and exits tiled mode
        state.remove_from_tile(0);
        assert!(!state.is_tiled());
        assert_eq!(state.selected_index, 2);
    }

    #[test]
    fn shift_select_range_and_tile_all() {
        let mut state = EditorState::new();
        state.new_tab();
        state.new_tab();
        assert_eq!(state.tabs.len(), 3);

        state.select_single_tab(0);
        state.select_tab_range(2);
        assert_eq!(state.selected_tabs, vec![0, 1, 2]);

        state.tile_selected_tabs();
        assert!(state.is_tiled());
        assert_eq!(state.tiled_tabs, vec![0, 1, 2]);

        state.untile_tabs();
        assert!(!state.is_tiled());
        assert_eq!(state.selected_tabs, vec![2]);
    }

    #[test]
    fn extract_insert_and_reorder_tabs_across_windows() {
        let mut win1 = EditorState::new();
        win1.tabs[0].apply_text("alpha".to_string());
        win1.new_tab();
        win1.tabs[1].apply_text("beta".to_string());
        win1.new_tab();
        win1.tabs[2].apply_text("gamma".to_string());

        // Reorder within the same window: move "alpha" (0) to index 2
        win1.move_tab_within(0, 2);
        let texts: Vec<&str> = win1.tabs.iter().map(|t| t.text.as_str()).collect();
        assert_eq!(texts, vec!["beta", "gamma", "alpha"]);

        // Detach "gamma" (index 1) into a new window via from_template_with_tabs
        let detached = win1.extract_tab(1, true).expect("should extract tab");
        assert_eq!(detached.text, "gamma");
        assert_eq!(win1.tabs.len(), 2);

        let mut win2 = win1.from_template_with_tabs(vec![detached]);
        assert_eq!(win2.tabs.len(), 1);
        assert_eq!(win2.tabs[0].text, "gamma");

        // Merge "gamma" from win2 back into win1 (allowing win2 to become empty)
        let merged = win2.extract_tab(0, false).expect("should extract last tab when allowed");
        assert!(win2.tabs.is_empty());
        win1.insert_tab(merged);
        assert_eq!(win1.tabs.len(), 3);
        assert_eq!(win1.tabs[2].text, "gamma");
        assert_eq!(win1.selected_index, 2);
    }
}
