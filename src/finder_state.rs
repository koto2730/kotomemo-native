#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FinderMode {
    Find,
    Replace,
}

/// All Find/Replace UI state. Ports Kotlin's `FinderState` minus the
/// `armed`/`focusTick` focus-blur workaround: that existed because
/// Compose's legacy `BasicTextField` fires a spurious `onValueChange` with
/// a collapsed selection as a side effect of losing focus (moving focus to
/// this bar's own query field). egui's `TextEdit` persists its cursor
/// range per-widget-id in its own memory store and doesn't reset it on
/// blur, so that specific failure mode shouldn't occur here — a
/// simplification to keep an eye on rather than a proven-safe port.
pub struct FinderState {
    pub visible: bool,
    pub mode: FinderMode,
    pub query: String,
    pub replacement: String,
    pub regex: bool,
    pub case_sensitive: bool,
    pub last_match_count: usize,
    /// -1 sentinel = "no replace has happened yet this session".
    pub last_replace_count: i64,
    pub wrapped_around: bool,
    /// Restricts Find/Replace to a char range instead of the whole
    /// document; `None` = whole document. Captured when the bar opens (or
    /// switches Find/Replace) and kept live afterward — the editor pane
    /// re-derives it from the live selection on every real selection-only
    /// change while the bar is visible.
    pub scope: Option<(usize, usize)>,
    /// Set true for exactly the frame after the bar opens or switches
    /// mode, so the UI can request focus on the query field once. Enter is
    /// deliberately *not* wired to re-request this (see
    /// `KotomemoApp::handle_shortcuts`) — a jump moves focus to the main
    /// editor so egui will actually paint the match highlighted (it only
    /// paints a `TextEdit`'s selection when that widget is focused), and
    /// pulling focus back to the query field for a follow-up Enter raced
    /// with that, intermittently leaving nothing focused and thus nothing
    /// highlighted. No Compose-style `focusTick` + `LaunchedEffect` needed
    /// here — the bar just reads and clears this itself each frame.
    pub refocus_query: bool,
}

impl Default for FinderState {
    fn default() -> Self {
        Self {
            visible: false,
            mode: FinderMode::Find,
            query: String::new(),
            replacement: String::new(),
            regex: false,
            case_sensitive: false,
            last_match_count: 0,
            last_replace_count: -1,
            wrapped_around: false,
            scope: None,
            refocus_query: false,
        }
    }
}

impl FinderState {
    pub fn toggle_find(&mut self, selection: Option<(usize, usize)>) {
        if self.visible && self.mode == FinderMode::Find {
            self.hide();
        } else {
            self.mode = FinderMode::Find;
            self.visible = true;
            self.refocus_query = true;
            self.scope = Self::scope_from(selection);
        }
    }

    pub fn toggle_replace(&mut self, selection: Option<(usize, usize)>) {
        if self.visible && self.mode == FinderMode::Replace {
            self.hide();
        } else {
            self.mode = FinderMode::Replace;
            self.visible = true;
            self.refocus_query = true;
            self.scope = Self::scope_from(selection);
        }
    }

    pub fn hide(&mut self) {
        self.visible = false;
        self.last_replace_count = -1;
        self.wrapped_around = false;
        self.scope = None;
    }

    fn scope_from(selection: Option<(usize, usize)>) -> Option<(usize, usize)> {
        selection.filter(|(start, end)| start != end)
    }
}
