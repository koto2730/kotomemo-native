use crate::editor_state::EditorState;

pub enum TabBarAction {
    None,
    Select(usize),
    ToggleSelect(usize),
    SelectRange(usize),
    Close(usize),
    New,
    TileSelected,
    TileAll,
    Untile,
    DetachToNewWindow(usize),
    MoveToWindow {
        tab_index: usize,
        target_viewport: egui::ViewportId,
    },
}

pub struct TabBarOutput {
    pub action: TabBarAction,
    pub bar_rect: egui::Rect,
    pub tab_rects: Vec<egui::Rect>,
    pub drag_started_tab: Option<usize>,
}

/// Horizontal tab strip:
/// - Click: select tab
/// - Ctrl+Click: toggle tab in multi-selection
/// - Shift+Click: select range of tabs
/// - Drag: reorder within tab bar, drag outside to detach into a new window,
///   or drag onto another window to merge into it
/// - Right-click or toolbar button: tile selected tabs, move to new/other window, or close
pub fn show(
    ui: &mut egui::Ui,
    state: &EditorState,
    other_windows: &[(egui::ViewportId, String)],
) -> TabBarOutput {
    let mut action = TabBarAction::None;
    let mut tab_rects = Vec::with_capacity(state.tabs.len());
    let mut drag_started_tab = None;

    let scroll_out = egui::ScrollArea::horizontal()
        .auto_shrink([false, true])
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let multi_count = state.selected_tabs.len();
                let is_tiled = state.is_tiled();

                for (i, tab) in state.tabs.iter().enumerate() {
                    let is_active = i == state.selected_index;
                    let is_in_multi = multi_count > 1 && state.selected_tabs.contains(&i);
                    let is_in_tile = is_tiled && state.tiled_tabs.contains(&i);

                    let mark = if tab.contents.is_dirty { " ●" } else { "" };
                    let prefix = if is_in_multi || is_in_tile { "✓ " } else { "" };
                    let label = format!("{prefix}{}{mark}", tab.contents.display_name());

                    let mut rich = egui::RichText::new(label);
                    if is_active {
                        rich = rich.strong();
                    }

                    let resp = ui
                        .add(
                            egui::Button::selectable(
                                is_active || is_in_multi || is_in_tile,
                                rich,
                            )
                            .sense(egui::Sense::click_and_drag()),
                        )
                        .on_hover_text(
                            "Click: select | Ctrl+Click: multi-select | Drag: move/detach to window | Right-click: options",
                        );

                    tab_rects.push(resp.rect);

                    if resp.drag_started_by(egui::PointerButton::Primary) {
                        drag_started_tab = Some(i);
                    }

                    if resp.clicked() {
                        let mods = ui.input(|inp| inp.modifiers);
                        if mods.command || mods.ctrl {
                            action = TabBarAction::ToggleSelect(i);
                        } else if mods.shift {
                            action = TabBarAction::SelectRange(i);
                        } else {
                            action = TabBarAction::Select(i);
                        }
                    }

                    resp.context_menu(|ui| {
                        let is_selected = state.selected_tabs.contains(&i);
                        let toggle_label = if is_selected && state.selected_tabs.len() > 1 {
                            "Deselect Tab (Ctrl+Click)"
                        } else {
                            "Add Tab to Selection (Ctrl+Click)"
                        };
                        if ui.button(toggle_label).clicked() {
                            action = TabBarAction::ToggleSelect(i);
                            ui.close();
                        }
                        if state.selected_tabs.len() >= 2 {
                            if ui
                                .button(format!("Tile Selected Tabs ({})", state.selected_tabs.len()))
                                .clicked()
                            {
                                action = TabBarAction::TileSelected;
                                ui.close();
                            }
                        }
                        if state.tabs.len() >= 2 {
                            if ui
                                .button(format!("Tile All Tabs ({})", state.tabs.len()))
                                .clicked()
                            {
                                action = TabBarAction::TileAll;
                                ui.close();
                            }
                        }
                        if state.is_tiled() {
                            if ui.button("Single Tab View (Untile)").clicked() {
                                action = TabBarAction::Untile;
                                ui.close();
                            }
                        }
                        ui.separator();
                        if ui
                            .add_enabled(
                                state.tabs.len() >= 2,
                                egui::Button::new("Move Tab to New Window"),
                            )
                            .clicked()
                        {
                            action = TabBarAction::DetachToNewWindow(i);
                            ui.close();
                        }
                        if !other_windows.is_empty() {
                            ui.menu_button("Move Tab to Window", |ui| {
                                for (target_vp, win_label) in other_windows {
                                    if ui.button(win_label).clicked() {
                                        action = TabBarAction::MoveToWindow {
                                            tab_index: i,
                                            target_viewport: *target_vp,
                                        };
                                        ui.close();
                                    }
                                }
                            });
                        }
                        ui.separator();
                        if ui.button("Close Tab").clicked() {
                            action = TabBarAction::Close(i);
                            ui.close();
                        }
                    });

                    if ui.small_button("×").clicked() {
                        action = TabBarAction::Close(i);
                    }
                    ui.separator();
                }

                if ui.button("+").on_hover_text("New Tab (Ctrl+N)").clicked() {
                    action = TabBarAction::New;
                }

                if state.selected_tabs.len() >= 2 && state.tiled_tabs != state.selected_tabs {
                    ui.separator();
                    if ui
                        .button(format!("⫴ Tile Tabs ({})", state.selected_tabs.len()))
                        .on_hover_text("Display selected tabs side-by-side")
                        .clicked()
                    {
                        action = TabBarAction::TileSelected;
                    }
                }

                if state.is_tiled() {
                    ui.separator();
                    if ui
                        .button(format!("⬜ Single View (from {})", state.tiled_tabs.len()))
                        .on_hover_text("Exit side-by-side view and return to single tab")
                        .clicked()
                    {
                        action = TabBarAction::Untile;
                    }
                }
            })
            .response
            .rect
        });

    TabBarOutput {
        action,
        bar_rect: scroll_out.inner,
        tab_rects,
        drag_started_tab,
    }
}
