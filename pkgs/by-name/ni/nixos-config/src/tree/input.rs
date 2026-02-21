use anyhow::Result;
use crossterm::event::{KeyCode, KeyModifiers, MouseEventKind, MouseButton};

use crate::theme::*;
use crate::tui;
use super::types::*;
use super::icons::*;
use super::data::*;

pub(super) enum InputAction {
    Continue,
    Quit,
}

pub(super) fn handle_input(
    input: &tui::InputEvent,
    mode: &mut Mode,
    state: &mut MillerState,
    root_children: &[(String, ConfigNode)],
    deps_index: &DepsIndex,
    status_msg: &mut Option<String>,
    pane_areas: &PaneAreas,
    terminal_height: u16,
    middle_count: usize,
) -> Result<InputAction> {
    // Handle mouse events
    if let tui::InputEvent::Mouse(mouse) = input {
        let col = mouse.column;
        let row = mouse.row;

        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                match mode {
                    Mode::Normal | Mode::Help => {
                        if matches!(mode, Mode::Help) {
                            *mode = Mode::Normal;
                            return Ok(InputAction::Continue);
                        }
                        // Browse pane: click to select item + focus
                        if rect_contains(pane_areas.browse_inner, col, row) {
                            let line = (row - pane_areas.browse_inner.y) as usize;
                            let idx = pane_areas.browse_scroll + line;
                            if idx < pane_areas.browse_count {
                                state.cursor = idx;
                                state.detail_scroll = 0;
                                state.deps_cursor = 0;
                                state.deps_scroll = 0;
                                state.focus = Focus::Middle;
                            }
                        }
                        // Parent pane: click to go up
                        else if rect_contains(pane_areas.parent_inner, col, row) && !state.path.is_empty() {
                            state.path_memory.insert(
                                state.path.clone(),
                                (state.cursor, state.scroll),
                            );
                            state.path.pop();
                            let (c, s) = state
                                .path_memory
                                .get(&state.path)
                                .copied()
                                .unwrap_or((0, 0));
                            state.cursor = c;
                            state.scroll = s;
                            state.detail_scroll = 0;
                            state.deps_cursor = 0;
                            state.deps_scroll = 0;
                            state.focus = Focus::Middle;
                        }
                        // Children pane: click to drill in
                        else if rect_contains(pane_areas.children_inner, col, row) {
                            if let Some(children) = get_children_at_path(root_children, &state.path) {
                                if let Some((name, ConfigNode::Branch(ch))) = children.get(state.cursor) {
                                    let line = (row - pane_areas.children_inner.y) as usize;
                                    if line < ch.len() {
                                        state.path_memory.insert(
                                            state.path.clone(),
                                            (state.cursor, state.scroll),
                                        );
                                        state.path.push(name.clone());
                                        state.cursor = line;
                                        state.scroll = 0;
                                        state.detail_scroll = 0;
                                        state.deps_cursor = 0;
                                        state.deps_scroll = 0;
                                        state.focus = Focus::Middle;
                                    }
                                }
                            }
                        }
                        // Detail pane: click to focus
                        else if rect_contains(pane_areas.detail_inner, col, row) {
                            state.focus = Focus::Detail;
                        }
                        // Deps pane: click to focus + select item, or jump if already selected
                        else if rect_contains(pane_areas.deps_inner, col, row) {
                            let line = (row - pane_areas.deps_inner.y) as usize;
                            let idx = pane_areas.deps_scroll + line;
                            if idx < pane_areas.deps_count {
                                if state.focus == Focus::Deps && state.deps_cursor == idx {
                                    let full_path = current_full_path(state, root_children);
                                    let path_str = full_path.join(".");
                                    if let Some(items) = deps_index.dependencies.get(&path_str) {
                                        if idx < items.len() {
                                            let target: Vec<String> = items[idx].split('.').map(|s| s.to_string()).collect();
                                            let msg = format!("Jumped to {}", target.join("."));
                                            jump_to_path(state, &target, root_children);
                                            *status_msg = Some(msg);
                                        }
                                    }
                                } else {
                                    state.deps_cursor = idx;
                                }
                            }
                            state.focus = Focus::Deps;
                        }
                        // Revs pane: click to focus + select item, or jump if already selected
                        else if rect_contains(pane_areas.revs_inner, col, row) {
                            let line = (row - pane_areas.revs_inner.y) as usize;
                            let idx = pane_areas.deps_scroll + line;
                            if idx < pane_areas.revs_count {
                                if state.focus == Focus::Revs && state.deps_cursor == idx {
                                    let full_path = current_full_path(state, root_children);
                                    let path_str = full_path.join(".");
                                    if let Some(items) = deps_index.dependents.get(&path_str) {
                                        if idx < items.len() {
                                            let target: Vec<String> = items[idx].split('.').map(|s| s.to_string()).collect();
                                            let msg = format!("Jumped to {}", target.join("."));
                                            jump_to_path(state, &target, root_children);
                                            *status_msg = Some(msg);
                                        }
                                    }
                                } else {
                                    state.deps_cursor = idx;
                                }
                            }
                            state.focus = Focus::Revs;
                        }
                    }
                    Mode::Search {
                        cursor: s_cursor,
                        results,
                        right_focus,
                        detail_scroll: s_detail_scroll,
                        deps_cursor: s_deps_cursor,
                        deps_scroll: s_deps_scroll,
                        ..
                    } => {
                        // Results pane
                        if rect_contains(pane_areas.search_results_inner, col, row) {
                            let line = (row - pane_areas.search_results_inner.y) as usize;
                            let idx = pane_areas.search_results_scroll + line;
                            if idx < pane_areas.search_results_count {
                                *s_cursor = idx;
                                *s_detail_scroll = 0;
                                *s_deps_cursor = 0;
                                *s_deps_scroll = 0;
                            }
                            *right_focus = Focus::Middle;
                        }
                        // Detail pane
                        else if rect_contains(pane_areas.search_detail_inner, col, row) {
                            *right_focus = Focus::Detail;
                        }
                        // Deps pane
                        else if rect_contains(pane_areas.search_deps_inner, col, row) {
                            let line = (row - pane_areas.search_deps_inner.y) as usize;
                            let idx = pane_areas.search_deps_scroll + line;
                            if idx < pane_areas.search_deps_count {
                                if *right_focus == Focus::Deps && *s_deps_cursor == idx {
                                    let path_str = if !results.is_empty() && *s_cursor < results.len() {
                                        results[*s_cursor].join(".")
                                    } else {
                                        String::new()
                                    };
                                    if let Some(items) = deps_index.dependencies.get(&path_str) {
                                        if idx < items.len() {
                                            let target: Vec<String> = items[idx].split('.').map(|s| s.to_string()).collect();
                                            let msg = format!("Jumped to {}", target.join("."));
                                            *mode = Mode::Normal;
                                            jump_to_path(state, &target, root_children);
                                            *status_msg = Some(msg);
                                            return Ok(InputAction::Continue);
                                        }
                                    }
                                } else {
                                    *s_deps_cursor = idx;
                                }
                            }
                            if let Mode::Search { right_focus, .. } = mode {
                                *right_focus = Focus::Deps;
                            }
                        }
                        // Revs pane
                        else if rect_contains(pane_areas.search_revs_inner, col, row) {
                            let line = (row - pane_areas.search_revs_inner.y) as usize;
                            let idx = pane_areas.search_deps_scroll + line;
                            if idx < pane_areas.search_revs_count {
                                if *right_focus == Focus::Revs && *s_deps_cursor == idx {
                                    let path_str = if !results.is_empty() && *s_cursor < results.len() {
                                        results[*s_cursor].join(".")
                                    } else {
                                        String::new()
                                    };
                                    if let Some(items) = deps_index.dependents.get(&path_str) {
                                        if idx < items.len() {
                                            let target: Vec<String> = items[idx].split('.').map(|s| s.to_string()).collect();
                                            let msg = format!("Jumped to {}", target.join("."));
                                            *mode = Mode::Normal;
                                            jump_to_path(state, &target, root_children);
                                            *status_msg = Some(msg);
                                            return Ok(InputAction::Continue);
                                        }
                                    }
                                } else {
                                    *s_deps_cursor = idx;
                                }
                            }
                            if let Mode::Search { right_focus, .. } = mode {
                                *right_focus = Focus::Revs;
                            }
                        }
                    }
                    Mode::Pager { .. } => {}
                }
                return Ok(InputAction::Continue);
            }
            MouseEventKind::ScrollDown => {
                match mode {
                    Mode::Normal | Mode::Help => {
                        if rect_contains(pane_areas.browse_inner, col, row) {
                            if state.focus == Focus::Middle && middle_count > 0 && state.cursor + 1 < middle_count {
                                state.cursor += 1;
                                state.detail_scroll = 0;
                                state.deps_cursor = 0;
                                state.deps_scroll = 0;
                            }
                        } else if rect_contains(pane_areas.detail_inner, col, row) {
                            state.detail_scroll += 1;
                        } else if rect_contains(pane_areas.deps_inner, col, row) {
                            let full_path = current_full_path(state, root_children);
                            let path_str = full_path.join(".");
                            let total = deps_index.dependencies.get(&path_str).map(|v| v.len()).unwrap_or(0);
                            if total > 0 && state.deps_cursor + 1 < total {
                                state.deps_cursor += 1;
                            }
                        } else if rect_contains(pane_areas.revs_inner, col, row) {
                            let full_path = current_full_path(state, root_children);
                            let path_str = full_path.join(".");
                            let total = deps_index.dependents.get(&path_str).map(|v| v.len()).unwrap_or(0);
                            if total > 0 && state.deps_cursor + 1 < total {
                                state.deps_cursor += 1;
                            }
                        }
                    }
                    Mode::Search { cursor: s_cursor, results, deps_cursor: s_deps_cursor, right_focus, .. } => {
                        if rect_contains(pane_areas.search_results_inner, col, row) {
                            if !results.is_empty() && *s_cursor + 1 < results.len() {
                                *s_cursor += 1;
                            }
                        } else if rect_contains(pane_areas.search_deps_inner, col, row) && *right_focus == Focus::Deps {
                            if pane_areas.search_deps_count > 0 && *s_deps_cursor + 1 < pane_areas.search_deps_count {
                                *s_deps_cursor += 1;
                            }
                        } else if rect_contains(pane_areas.search_revs_inner, col, row) && *right_focus == Focus::Revs {
                            if pane_areas.search_revs_count > 0 && *s_deps_cursor + 1 < pane_areas.search_revs_count {
                                *s_deps_cursor += 1;
                            }
                        }
                    }
                    Mode::Pager { scroll: p_scroll, lines: pager_lines, .. } => {
                        if *p_scroll + 1 < pager_lines.len() {
                            *p_scroll += 1;
                        }
                    }
                }
                return Ok(InputAction::Continue);
            }
            MouseEventKind::ScrollUp => {
                match mode {
                    Mode::Normal | Mode::Help => {
                        if rect_contains(pane_areas.browse_inner, col, row) {
                            if state.cursor > 0 {
                                state.cursor -= 1;
                                state.detail_scroll = 0;
                                state.deps_cursor = 0;
                                state.deps_scroll = 0;
                            }
                        } else if rect_contains(pane_areas.detail_inner, col, row) {
                            state.detail_scroll = state.detail_scroll.saturating_sub(1);
                        } else if rect_contains(pane_areas.deps_inner, col, row) {
                            state.deps_cursor = state.deps_cursor.saturating_sub(1);
                        } else if rect_contains(pane_areas.revs_inner, col, row) {
                            state.deps_cursor = state.deps_cursor.saturating_sub(1);
                        }
                    }
                    Mode::Search { cursor: s_cursor, deps_cursor: s_deps_cursor, right_focus, .. } => {
                        if rect_contains(pane_areas.search_results_inner, col, row) {
                            *s_cursor = s_cursor.saturating_sub(1);
                        } else if rect_contains(pane_areas.search_deps_inner, col, row) && *right_focus == Focus::Deps {
                            *s_deps_cursor = s_deps_cursor.saturating_sub(1);
                        } else if rect_contains(pane_areas.search_revs_inner, col, row) && *right_focus == Focus::Revs {
                            *s_deps_cursor = s_deps_cursor.saturating_sub(1);
                        }
                    }
                    Mode::Pager { scroll: p_scroll, .. } => {
                        *p_scroll = p_scroll.saturating_sub(1);
                    }
                }
                return Ok(InputAction::Continue);
            }
            _ => { return Ok(InputAction::Continue); }
        }
    }

    let key = match input {
        tui::InputEvent::Key(k) => k,
        _ => return Ok(InputAction::Continue),
    };

    // Global Ctrl-C quit from any mode
    if key.code == KeyCode::Char('c')
        && key.modifiers.contains(KeyModifiers::CONTROL)
    {
        return Ok(InputAction::Quit);
    }

    match mode {
        Mode::Help => match key.code {
            KeyCode::Char('?') | KeyCode::Esc | KeyCode::Char('q') => {
                *mode = Mode::Normal;
            }
            _ => {}
        },

        Mode::Pager {
            lines: pager_lines,
            scroll: p_scroll,
            ..
        } => match key.code {
            KeyCode::Char('j') | KeyCode::Down => {
                if *p_scroll + 1 < pager_lines.len() {
                    *p_scroll += 1;
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                *p_scroll = p_scroll.saturating_sub(1);
            }
            KeyCode::Char('g') | KeyCode::Home => {
                *p_scroll = 0;
            }
            KeyCode::Char('G') | KeyCode::End => {
                *p_scroll = pager_lines.len().saturating_sub(1);
            }
            KeyCode::PageDown => {
                let page = terminal_height.saturating_sub(4) as usize;
                *p_scroll =
                    (*p_scroll + page).min(pager_lines.len().saturating_sub(1));
            }
            KeyCode::PageUp => {
                let page = terminal_height.saturating_sub(4) as usize;
                *p_scroll = p_scroll.saturating_sub(page);
            }
            KeyCode::Esc
            | KeyCode::Char('q')
            | KeyCode::Char('h')
            | KeyCode::Left
            | KeyCode::Backspace => {
                *mode = Mode::Normal;
            }
            _ => {}
        },

        Mode::Normal => {
            *status_msg = None;
            if key.code == KeyCode::Char('q') {
                return Ok(InputAction::Quit);
            }

            match state.focus {
                Focus::Middle => match key.code {
                    KeyCode::Char('j') | KeyCode::Down => {
                        if middle_count > 0 && state.cursor + 1 < middle_count {
                            state.cursor += 1;
                            state.detail_scroll = 0;
                            state.deps_cursor = 0;
                            state.deps_scroll = 0;
                        }
                    }
                    KeyCode::Char('k') | KeyCode::Up => {
                        if state.cursor > 0 {
                            state.cursor -= 1;
                            state.detail_scroll = 0;
                            state.deps_cursor = 0;
                            state.deps_scroll = 0;
                        }
                    }
                    KeyCode::Char('g') | KeyCode::Home => {
                        state.cursor = 0;
                        state.detail_scroll = 0;
                        state.deps_cursor = 0;
                        state.deps_scroll = 0;
                    }
                    KeyCode::Char('G') | KeyCode::End => {
                        state.cursor = middle_count.saturating_sub(1);
                        state.detail_scroll = 0;
                        state.deps_cursor = 0;
                        state.deps_scroll = 0;
                    }
                    KeyCode::PageDown => {
                        let page = terminal_height.saturating_sub(6) as usize;
                        state.cursor =
                            (state.cursor + page).min(middle_count.saturating_sub(1));
                        state.detail_scroll = 0;
                        state.deps_cursor = 0;
                        state.deps_scroll = 0;
                    }
                    KeyCode::PageUp => {
                        let page = terminal_height.saturating_sub(6) as usize;
                        state.cursor = state.cursor.saturating_sub(page);
                        state.detail_scroll = 0;
                        state.deps_cursor = 0;
                        state.deps_scroll = 0;
                    }
                    KeyCode::Char('l') | KeyCode::Right | KeyCode::Enter => {
                        if let Some(children) =
                            get_children_at_path(root_children, &state.path)
                        {
                            if let Some((name, node)) = children.get(state.cursor) {
                                match node {
                                    ConfigNode::Branch(_) => {
                                        state.path_memory.insert(
                                            state.path.clone(),
                                            (state.cursor, state.scroll),
                                        );
                                        state.path.push(name.clone());
                                        let (c, s) = state
                                            .path_memory
                                            .get(&state.path)
                                            .copied()
                                            .unwrap_or((0, 0));
                                        state.cursor = c;
                                        state.scroll = s;
                                        state.detail_scroll = 0;
                                        state.deps_cursor = 0;
                                        state.deps_scroll = 0;
                                    }
                                    ConfigNode::Leaf(val) => {
                                        let mut full = state.path.clone();
                                        full.push(name.clone());
                                        let lines = format_value_full(val);
                                        let color = value_color(val);
                                        *mode = Mode::Pager {
                                            path: full,
                                            lines,
                                            scroll: 0,
                                            color,
                                        };
                                    }
                                    ConfigNode::Phantom => {
                                        let mut full = state.path.clone();
                                        full.push(name.clone());
                                        *mode = Mode::Pager {
                                            path: full,
                                            lines: vec!["! (not serializable or value not used)".to_string()],
                                            scroll: 0,
                                            color: YELLOW,
                                        };
                                    }
                                }
                            }
                        }
                    }
                    KeyCode::Char('h') | KeyCode::Left | KeyCode::Esc => {
                        if !state.path.is_empty() {
                            state.path_memory.insert(
                                state.path.clone(),
                                (state.cursor, state.scroll),
                            );
                            state.path.pop();
                            let (c, s) = state
                                .path_memory
                                .get(&state.path)
                                .copied()
                                .unwrap_or((0, 0));
                            state.cursor = c;
                            state.scroll = s;
                            state.detail_scroll = 0;
                            state.deps_cursor = 0;
                            state.deps_scroll = 0;
                        }
                    }
                    KeyCode::Char('d') => {
                        state.focus = Focus::Detail;
                    }
                    KeyCode::Char('p') => {
                        state.focus = Focus::Deps;
                        state.deps_cursor = 0;
                        state.deps_scroll = 0;
                    }
                    KeyCode::Char('n') => {
                        state.focus = Focus::Revs;
                        state.deps_cursor = 0;
                        state.deps_scroll = 0;
                    }
                    KeyCode::Char('J') => {
                        state.detail_scroll += 1;
                    }
                    KeyCode::Char('K') => {
                        state.detail_scroll = state.detail_scroll.saturating_sub(1);
                    }
                    KeyCode::Char('/') => {
                        *mode = Mode::Search {
                            query: String::new(),
                            results: Vec::new(),
                            cursor: 0,
                            scroll: 0,
                            right_focus: Focus::Middle,
                            detail_scroll: 0,
                            deps_cursor: 0,
                            deps_scroll: 0,
                        };
                    }
                    KeyCode::Char('?') => {
                        *mode = Mode::Help;
                    }
                    _ => {}
                },

                Focus::Detail => match key.code {
                    KeyCode::Char('j') | KeyCode::Down => {
                        state.detail_scroll += 1;
                    }
                    KeyCode::Char('k') | KeyCode::Up => {
                        state.detail_scroll =
                            state.detail_scroll.saturating_sub(1);
                    }
                    KeyCode::PageDown => {
                        let page =
                            terminal_height.saturating_sub(8) as usize;
                        state.detail_scroll += page;
                    }
                    KeyCode::PageUp => {
                        let page =
                            terminal_height.saturating_sub(8) as usize;
                        state.detail_scroll =
                            state.detail_scroll.saturating_sub(page);
                    }
                    KeyCode::Esc | KeyCode::Char('h') | KeyCode::Left
                    | KeyCode::Char('b') => {
                        state.focus = Focus::Middle;
                    }
                    KeyCode::Char('p') => {
                        state.focus = Focus::Deps;
                        state.deps_cursor = 0;
                        state.deps_scroll = 0;
                    }
                    KeyCode::Char('n') => {
                        state.focus = Focus::Revs;
                        state.deps_cursor = 0;
                        state.deps_scroll = 0;
                    }
                    _ => {}
                },

                Focus::Deps => {
                    let full_path = current_full_path(state, root_children);
                    let path_str = full_path.join(".");
                    let dep_items: Vec<String> = deps_index
                        .dependencies
                        .get(&path_str)
                        .cloned()
                        .unwrap_or_default();
                    let total = dep_items.len();

                    match key.code {
                        KeyCode::Char('j') | KeyCode::Down => {
                            if total > 0 && state.deps_cursor + 1 < total {
                                state.deps_cursor += 1;
                            }
                        }
                        KeyCode::Char('k') | KeyCode::Up => {
                            state.deps_cursor =
                                state.deps_cursor.saturating_sub(1);
                        }
                        KeyCode::PageDown => {
                            let page =
                                terminal_height.saturating_sub(8) as usize;
                            state.deps_cursor = (state.deps_cursor + page)
                                .min(total.saturating_sub(1));
                        }
                        KeyCode::PageUp => {
                            let page =
                                terminal_height.saturating_sub(8) as usize;
                            state.deps_cursor =
                                state.deps_cursor.saturating_sub(page);
                        }
                        KeyCode::Enter => {
                            if state.deps_cursor < total {
                                let target: Vec<String> = dep_items
                                    [state.deps_cursor]
                                    .split('.')
                                    .map(|s| s.to_string())
                                    .collect();
                                let msg =
                                    format!("Jumped to {}", target.join("."));
                                jump_to_path(
                                    state,
                                    &target,
                                    root_children,
                                );
                                *status_msg = Some(msg);
                            }
                        }
                        KeyCode::Esc | KeyCode::Char('h') | KeyCode::Left
                        | KeyCode::Char('b') => {
                            state.focus = Focus::Middle;
                        }
                        KeyCode::Char('d') => {
                            state.focus = Focus::Detail;
                        }
                        KeyCode::Char('n') => {
                            state.focus = Focus::Revs;
                            state.deps_cursor = 0;
                            state.deps_scroll = 0;
                        }
                        _ => {}
                    }
                }

                Focus::Revs => {
                    let full_path = current_full_path(state, root_children);
                    let path_str = full_path.join(".");
                    let rev_items: Vec<String> = deps_index
                        .dependents
                        .get(&path_str)
                        .cloned()
                        .unwrap_or_default();
                    let total = rev_items.len();

                    match key.code {
                        KeyCode::Char('j') | KeyCode::Down => {
                            if total > 0 && state.deps_cursor + 1 < total {
                                state.deps_cursor += 1;
                            }
                        }
                        KeyCode::Char('k') | KeyCode::Up => {
                            state.deps_cursor =
                                state.deps_cursor.saturating_sub(1);
                        }
                        KeyCode::PageDown => {
                            let page =
                                terminal_height.saturating_sub(8) as usize;
                            state.deps_cursor = (state.deps_cursor + page)
                                .min(total.saturating_sub(1));
                        }
                        KeyCode::PageUp => {
                            let page =
                                terminal_height.saturating_sub(8) as usize;
                            state.deps_cursor =
                                state.deps_cursor.saturating_sub(page);
                        }
                        KeyCode::Enter => {
                            if state.deps_cursor < total {
                                let target: Vec<String> = rev_items
                                    [state.deps_cursor]
                                    .split('.')
                                    .map(|s| s.to_string())
                                    .collect();
                                let msg =
                                    format!("Jumped to {}", target.join("."));
                                jump_to_path(
                                    state,
                                    &target,
                                    root_children,
                                );
                                *status_msg = Some(msg);
                            }
                        }
                        KeyCode::Esc | KeyCode::Char('h') | KeyCode::Left
                        | KeyCode::Char('b') => {
                            state.focus = Focus::Middle;
                        }
                        KeyCode::Char('d') => {
                            state.focus = Focus::Detail;
                        }
                        KeyCode::Char('p') => {
                            state.focus = Focus::Deps;
                            state.deps_cursor = 0;
                            state.deps_scroll = 0;
                        }
                        _ => {}
                    }
                }
            }
        }

        Mode::Search {
            query,
            results,
            cursor: s_cursor,
            scroll: s_scroll,
            right_focus,
            detail_scroll: s_detail_scroll,
            deps_cursor: s_deps_cursor,
            deps_scroll: s_deps_scroll,
        } => match right_focus {
            Focus::Middle => match key.code {
                KeyCode::Esc => {
                    *mode = Mode::Normal;
                }
                KeyCode::Enter => {
                    if !results.is_empty() && *s_cursor < results.len() {
                        let target = results[*s_cursor].clone();
                        *mode = Mode::Normal;
                        jump_to_path(state, &target, root_children);
                        *status_msg = Some(format!("Jumped to {}", target.join(".")));
                    }
                }
                KeyCode::Tab => {
                    *right_focus = Focus::Detail;
                }
                KeyCode::Down => {
                    if !results.is_empty() && *s_cursor + 1 < results.len() {
                        *s_cursor += 1;
                        *s_detail_scroll = 0;
                        *s_deps_cursor = 0;
                        *s_deps_scroll = 0;
                    }
                }
                KeyCode::Up => {
                    if *s_cursor > 0 {
                        *s_cursor -= 1;
                        *s_detail_scroll = 0;
                        *s_deps_cursor = 0;
                        *s_deps_scroll = 0;
                    }
                }
                KeyCode::PageDown => {
                    let page = terminal_height.saturating_sub(8) as usize;
                    *s_cursor =
                        (*s_cursor + page).min(results.len().saturating_sub(1));
                    *s_detail_scroll = 0;
                    *s_deps_cursor = 0;
                    *s_deps_scroll = 0;
                }
                KeyCode::PageUp => {
                    let page = terminal_height.saturating_sub(8) as usize;
                    *s_cursor = s_cursor.saturating_sub(page);
                    *s_detail_scroll = 0;
                    *s_deps_cursor = 0;
                    *s_deps_scroll = 0;
                }
                KeyCode::Backspace => {
                    query.pop();
                    *results = search_tree(root_children, query, &[]);
                    *s_cursor = 0;
                    *s_scroll = 0;
                    *s_detail_scroll = 0;
                    *s_deps_cursor = 0;
                    *s_deps_scroll = 0;
                }
                KeyCode::Char(c) => {
                    query.push(c);
                    *results = search_tree(root_children, query, &[]);
                    *s_cursor = 0;
                    *s_scroll = 0;
                    *s_detail_scroll = 0;
                    *s_deps_cursor = 0;
                    *s_deps_scroll = 0;
                }
                _ => {}
            },
            Focus::Detail => match key.code {
                KeyCode::Char('j') | KeyCode::Down => {
                    *s_detail_scroll += 1;
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    *s_detail_scroll = s_detail_scroll.saturating_sub(1);
                }
                KeyCode::PageDown => {
                    let page = terminal_height.saturating_sub(8) as usize;
                    *s_detail_scroll += page;
                }
                KeyCode::PageUp => {
                    let page = terminal_height.saturating_sub(8) as usize;
                    *s_detail_scroll = s_detail_scroll.saturating_sub(page);
                }
                KeyCode::Char('p') => {
                    *right_focus = Focus::Deps;
                    *s_deps_cursor = 0;
                    *s_deps_scroll = 0;
                }
                KeyCode::Char('n') => {
                    *right_focus = Focus::Revs;
                    *s_deps_cursor = 0;
                    *s_deps_scroll = 0;
                }
                KeyCode::Esc | KeyCode::Char('h') | KeyCode::Tab => {
                    *right_focus = Focus::Middle;
                }
                _ => {}
            },
            Focus::Deps => {
                let path_str = if !results.is_empty() && *s_cursor < results.len() {
                    results[*s_cursor].join(".")
                } else {
                    String::new()
                };
                let dep_items: Vec<String> = deps_index
                    .dependencies
                    .get(&path_str)
                    .cloned()
                    .unwrap_or_default();
                let dep_total = dep_items.len();

                match key.code {
                    KeyCode::Char('j') | KeyCode::Down => {
                        if dep_total > 0 && *s_deps_cursor + 1 < dep_total {
                            *s_deps_cursor += 1;
                        }
                    }
                    KeyCode::Char('k') | KeyCode::Up => {
                        *s_deps_cursor = s_deps_cursor.saturating_sub(1);
                    }
                    KeyCode::PageDown => {
                        let page = terminal_height.saturating_sub(8) as usize;
                        *s_deps_cursor = (*s_deps_cursor + page).min(dep_total.saturating_sub(1));
                    }
                    KeyCode::PageUp => {
                        let page = terminal_height.saturating_sub(8) as usize;
                        *s_deps_cursor = s_deps_cursor.saturating_sub(page);
                    }
                    KeyCode::Enter => {
                        if *s_deps_cursor < dep_total {
                            let target: Vec<String> = dep_items[*s_deps_cursor]
                                .split('.')
                                .map(|s| s.to_string())
                                .collect();
                            let msg = format!("Jumped to {}", target.join("."));
                            *mode = Mode::Normal;
                            jump_to_path(state, &target, root_children);
                            *status_msg = Some(msg);
                        }
                    }
                    KeyCode::Char('d') => {
                        *right_focus = Focus::Detail;
                    }
                    KeyCode::Char('n') => {
                        *right_focus = Focus::Revs;
                        *s_deps_cursor = 0;
                        *s_deps_scroll = 0;
                    }
                    KeyCode::Esc | KeyCode::Char('h') | KeyCode::Tab => {
                        *right_focus = Focus::Middle;
                    }
                    _ => {}
                }
            }
            Focus::Revs => {
                let path_str = if !results.is_empty() && *s_cursor < results.len() {
                    results[*s_cursor].join(".")
                } else {
                    String::new()
                };
                let rev_items: Vec<String> = deps_index
                    .dependents
                    .get(&path_str)
                    .cloned()
                    .unwrap_or_default();
                let rev_total = rev_items.len();

                match key.code {
                    KeyCode::Char('j') | KeyCode::Down => {
                        if rev_total > 0 && *s_deps_cursor + 1 < rev_total {
                            *s_deps_cursor += 1;
                        }
                    }
                    KeyCode::Char('k') | KeyCode::Up => {
                        *s_deps_cursor = s_deps_cursor.saturating_sub(1);
                    }
                    KeyCode::PageDown => {
                        let page = terminal_height.saturating_sub(8) as usize;
                        *s_deps_cursor = (*s_deps_cursor + page).min(rev_total.saturating_sub(1));
                    }
                    KeyCode::PageUp => {
                        let page = terminal_height.saturating_sub(8) as usize;
                        *s_deps_cursor = s_deps_cursor.saturating_sub(page);
                    }
                    KeyCode::Enter => {
                        if *s_deps_cursor < rev_total {
                            let target: Vec<String> = rev_items[*s_deps_cursor]
                                .split('.')
                                .map(|s| s.to_string())
                                .collect();
                            let msg = format!("Jumped to {}", target.join("."));
                            *mode = Mode::Normal;
                            jump_to_path(state, &target, root_children);
                            *status_msg = Some(msg);
                        }
                    }
                    KeyCode::Char('d') => {
                        *right_focus = Focus::Detail;
                    }
                    KeyCode::Char('p') => {
                        *right_focus = Focus::Deps;
                        *s_deps_cursor = 0;
                        *s_deps_scroll = 0;
                    }
                    KeyCode::Esc | KeyCode::Char('h') | KeyCode::Tab => {
                        *right_focus = Focus::Middle;
                    }
                    _ => {}
                }
            }
        },
    }

    Ok(InputAction::Continue)
}
