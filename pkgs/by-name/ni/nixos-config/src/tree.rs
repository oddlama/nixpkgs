use std::collections::HashMap;

use anyhow::Result;
use crossterm::event::KeyCode;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph};
use serde_json::Value;

use crate::resolve;
use crate::tui;

// ---------------------------------------------------------------------------
// Data structures
// ---------------------------------------------------------------------------

enum ConfigNode {
    Branch(Vec<(String, ConfigNode)>),
    Leaf(Value),
}

struct DepsIndex {
    dependencies: HashMap<String, Vec<String>>,
    dependents: HashMap<String, Vec<String>>,
}

#[derive(Clone, PartialEq)]
enum Focus {
    Middle,
    Detail,
}

struct MillerState {
    path: Vec<String>,
    cursor: usize,
    scroll: usize,
    cursor_stack: Vec<(usize, usize)>,
    right_scroll: usize,
    detail_cursor: usize,
    focus: Focus,
    force_detail: bool,
}

enum Mode {
    Normal,
    Search,
    SearchResults,
}

// ---------------------------------------------------------------------------
// Color system
// ---------------------------------------------------------------------------

fn section_color(name: &str) -> Color {
    match name {
        "services" => Color::Rgb(0x4e, 0x79, 0xa7),
        "systemd" => Color::Rgb(0x59, 0xa1, 0x4f),
        "boot" => Color::Rgb(0xe1, 0x57, 0x59),
        "networking" => Color::Rgb(0xf2, 0x8e, 0x2b),
        "users" => Color::Rgb(0xb0, 0x7a, 0xa1),
        "security" => Color::Rgb(0xff, 0x9d, 0xa7),
        "environment" => Color::Rgb(0x9c, 0x75, 0x5f),
        "hardware" => Color::Rgb(0xba, 0xb0, 0xac),
        "system" => Color::Rgb(0x76, 0xb7, 0xb2),
        "nix" | "nixpkgs" => Color::Rgb(0xed, 0xc9, 0x48),
        "programs" => Color::Rgb(0xaf, 0x7a, 0xa1),
        "fileSystems" => Color::Rgb(0xd4, 0xa3, 0x73),
        "virtualisation" => Color::Rgb(0x8c, 0xd1, 0x7d),
        _ => Color::Rgb(0xd3, 0xd3, 0xd3),
    }
}

fn value_color(value: &Value) -> Color {
    match value {
        Value::Bool(true) => Color::Rgb(0x59, 0xa1, 0x4f),
        Value::Bool(false) => Color::Rgb(0xe1, 0x57, 0x59),
        Value::Number(_) => Color::Rgb(0x00, 0xd7, 0xff),
        Value::String(_) => Color::Rgb(0xed, 0xc9, 0x48),
        Value::Null => Color::Rgb(0x88, 0x88, 0x88),
        _ => Color::Rgb(0xaa, 0xaa, 0xaa),
    }
}

fn top_level_section(path: &[String], name: &str) -> String {
    if path.is_empty() {
        name.to_string()
    } else {
        path[0].clone()
    }
}

// ---------------------------------------------------------------------------
// Tree building
// ---------------------------------------------------------------------------

fn build_config_tree(value: &Value) -> ConfigNode {
    match value {
        Value::Object(map) if map.is_empty() => ConfigNode::Leaf(value.clone()),
        Value::Object(map) => {
            let mut children: Vec<(String, ConfigNode)> = map
                .iter()
                .map(|(k, v)| (k.clone(), build_config_tree(v)))
                .collect();
            children.sort_by(|(a, _), (b, _)| a.cmp(b));
            ConfigNode::Branch(children)
        }
        _ => ConfigNode::Leaf(value.clone()),
    }
}

fn get_children_at_path<'a>(
    root: &'a [(String, ConfigNode)],
    path: &[String],
) -> Option<&'a [(String, ConfigNode)]> {
    let mut current = root;
    for segment in path {
        let idx = current.iter().position(|(n, _)| n == segment)?;
        match &current[idx].1 {
            ConfigNode::Branch(children) => current = children,
            ConfigNode::Leaf(_) => return None,
        }
    }
    Some(current)
}

fn get_node_at_path<'a>(
    root: &'a [(String, ConfigNode)],
    path: &[String],
    name: &str,
) -> Option<&'a ConfigNode> {
    let children = get_children_at_path(root, path)?;
    children.iter().find(|(n, _)| n == name).map(|(_, node)| node)
}

// ---------------------------------------------------------------------------
// Dependency index
// ---------------------------------------------------------------------------

fn build_deps_index(deps_json: &Value) -> DepsIndex {
    let mut dependencies: HashMap<String, Vec<String>> = HashMap::new();
    let mut dependents: HashMap<String, Vec<String>> = HashMap::new();

    if let Value::Array(entries) = deps_json {
        for entry in entries {
            let accessor = entry
                .get("accessor")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|s| s.as_str())
                        .collect::<Vec<_>>()
                        .join(".")
                });
            let accessed = entry
                .get("accessed")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|s| s.as_str())
                        .collect::<Vec<_>>()
                        .join(".")
                });

            if let (Some(accessor), Some(accessed)) = (accessor, accessed) {
                dependencies
                    .entry(accessor.clone())
                    .or_default()
                    .push(accessed.clone());
                dependents
                    .entry(accessed)
                    .or_default()
                    .push(accessor);
            }
        }
    }

    // Sort and dedup
    for v in dependencies.values_mut() {
        v.sort();
        v.dedup();
    }
    for v in dependents.values_mut() {
        v.sort();
        v.dedup();
    }

    DepsIndex {
        dependencies,
        dependents,
    }
}

// ---------------------------------------------------------------------------
// Value formatting
// ---------------------------------------------------------------------------

fn format_value_short(value: &Value) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => {
            if s.contains('\n') {
                "''...''".to_string()
            } else if s.len() > 40 {
                format!("\"{}...\"", &s[..37])
            } else {
                format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
            }
        }
        Value::Array(arr) => format!("[ ... ] ({} items)", arr.len()),
        Value::Object(map) if map.is_empty() => "{ }".to_string(),
        Value::Object(map) => format!("{{ ... }} ({} keys)", map.len()),
    }
}

fn format_value_full(value: &Value) -> Vec<String> {
    match value {
        Value::Null => vec!["null".to_string()],
        Value::Bool(b) => vec![b.to_string()],
        Value::Number(n) => vec![n.to_string()],
        Value::String(s) => {
            if s.contains('\n') {
                let mut lines = vec!["''".to_string()];
                for line in s.lines() {
                    lines.push(format!("  {}", line));
                }
                lines.push("''".to_string());
                lines
            } else {
                vec![format!(
                    "\"{}\"",
                    s.replace('\\', "\\\\").replace('"', "\\\"")
                )]
            }
        }
        Value::Array(arr) => {
            if arr.is_empty() {
                return vec!["[ ]".to_string()];
            }
            let mut lines = vec!["[".to_string()];
            for item in arr {
                lines.push(format!("  {}", format_value_short(item)));
            }
            lines.push("]".to_string());
            lines
        }
        Value::Object(map) => {
            if map.is_empty() {
                return vec!["{ }".to_string()];
            }
            let mut lines = vec!["{".to_string()];
            for (k, v) in map {
                lines.push(format!("  {} = {};", k, format_value_short(v)));
            }
            lines.push("}".to_string());
            lines
        }
    }
}

// ---------------------------------------------------------------------------
// Search
// ---------------------------------------------------------------------------

fn search_tree(
    root: &[(String, ConfigNode)],
    query: &str,
    prefix: &[String],
) -> Vec<Vec<String>> {
    let mut results = Vec::new();
    let lower_query = query.to_lowercase();
    for (name, node) in root {
        let mut full_path = prefix.to_vec();
        full_path.push(name.clone());
        if name.to_lowercase().contains(&lower_query) {
            results.push(full_path.clone());
        }
        if let ConfigNode::Branch(children) = node {
            results.extend(search_tree(children, query, &full_path));
        }
    }
    results
}

// ---------------------------------------------------------------------------
// Navigation helpers
// ---------------------------------------------------------------------------

fn jump_to_path(state: &mut MillerState, target_path: &[String], root: &[(String, ConfigNode)]) {
    if target_path.is_empty() {
        return;
    }

    state.path.clear();
    state.cursor_stack.clear();
    state.scroll = 0;
    state.right_scroll = 0;
    state.detail_cursor = 0;
    state.force_detail = false;

    let parent = &target_path[..target_path.len() - 1];
    let leaf = &target_path[target_path.len() - 1];

    let mut current = root;
    for segment in parent {
        if let Some(idx) = current.iter().position(|(n, _)| n == segment) {
            state.cursor_stack.push((idx, 0));
            state.path.push(segment.clone());
            match &current[idx].1 {
                ConfigNode::Branch(children) => current = children.as_slice(),
                _ => return,
            }
        } else {
            return;
        }
    }

    state.cursor = current.iter().position(|(n, _)| n == leaf).unwrap_or(0);
    state.focus = Focus::Middle;
}

fn clamp_cursor(cursor: &mut usize, scroll: &mut usize, total: usize, visible: usize) {
    if total == 0 {
        *cursor = 0;
        *scroll = 0;
        return;
    }
    if *cursor >= total {
        *cursor = total - 1;
    }
    if *cursor < *scroll {
        *scroll = *cursor;
    } else if *cursor >= *scroll + visible {
        *scroll = *cursor - visible + 1;
    }
}

// ---------------------------------------------------------------------------
// Rendering helpers
// ---------------------------------------------------------------------------

const ACTIVE_BORDER: Color = Color::Rgb(0x88, 0xaa, 0xff);
const INACTIVE_BORDER: Color = Color::Rgb(0x44, 0x44, 0x55);
const CURSOR_BG: Color = Color::Rgb(40, 40, 80);
const PARENT_HIGHLIGHT_BG: Color = Color::Rgb(30, 30, 50);
const DIM_GRAY: Color = Color::Rgb(0x66, 0x66, 0x66);

fn make_block(title: &str, active: bool) -> Block<'_> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(if active { ACTIVE_BORDER } else { INACTIVE_BORDER }))
        .title(Span::styled(
            format!(" {} ", title),
            Style::default()
                .fg(if active { ACTIVE_BORDER } else { INACTIVE_BORDER })
                .add_modifier(Modifier::BOLD),
        ))
}

fn render_pane_list<'a>(
    children: &[(String, ConfigNode)],
    state_path: &[String],
    highlight_name: Option<&str>,
    cursor_idx: Option<usize>,
    scroll: usize,
    visible_height: usize,
    inner_width: u16,
) -> Vec<Line<'a>> {
    let width = inner_width as usize;
    let end = children.len().min(scroll + visible_height);
    let mut lines = Vec::new();

    for i in scroll..end {
        let (name, node) = &children[i];
        let section = top_level_section(state_path, name);
        let color = section_color(&section);

        let is_cursor = cursor_idx == Some(i);
        let is_highlight = highlight_name == Some(name.as_str());

        let bg = if is_cursor {
            CURSOR_BG
        } else if is_highlight {
            PARENT_HIGHLIGHT_BG
        } else {
            Color::Reset
        };

        let name_style = Style::default().fg(color).bg(bg).add_modifier(
            if is_cursor || is_highlight {
                Modifier::BOLD
            } else {
                Modifier::empty()
            },
        );

        let suffix = match node {
            ConfigNode::Branch(ch) => {
                let count_str = format!("{}", ch.len());
                (count_str, Style::default().fg(DIM_GRAY).bg(bg))
            }
            ConfigNode::Leaf(val) => {
                let short = format!("= {}", format_value_short(val));
                let short = if short.len() > width / 2 {
                    format!("{}...", &short[..width / 2 - 3])
                } else {
                    short
                };
                (short, Style::default().fg(value_color(val)).bg(bg))
            }
        };

        let name_display = if name.len() + 1 + suffix.0.len() > width && name.len() > 3 {
            let max_name = width.saturating_sub(suffix.0.len() + 4);
            if max_name > 3 {
                format!("{}...", &name[..max_name])
            } else {
                name.clone()
            }
        } else {
            name.clone()
        };

        let name_len = name_display.len();
        let suffix_len = suffix.0.len();
        let padding = if name_len + suffix_len + 1 < width {
            width - name_len - suffix_len
        } else {
            1
        };

        lines.push(Line::from(vec![
            Span::styled(name_display, name_style),
            Span::styled(" ".repeat(padding), Style::default().bg(bg)),
            Span::styled(suffix.0, suffix.1),
        ]));
    }

    // Fill remaining lines
    for _ in lines.len()..visible_height {
        lines.push(Line::from(""));
    }

    lines
}

fn render_detail_pane<'a>(
    full_path: &[String],
    node: &ConfigNode,
    deps_index: &DepsIndex,
    detail_cursor: usize,
    right_scroll: usize,
    focus_detail: bool,
    inner_width: u16,
    visible_height: usize,
) -> Vec<Line<'a>> {
    let width = inner_width as usize;
    let path_str = full_path.join(".");
    let section = if full_path.is_empty() {
        ""
    } else {
        &full_path[0]
    };
    let path_color = section_color(section);

    let mut content: Vec<Line<'a>> = Vec::new();

    // Path header
    content.push(Line::from(Span::styled(
        path_str.clone(),
        Style::default()
            .fg(path_color)
            .add_modifier(Modifier::BOLD),
    )));
    content.push(Line::from(""));

    // Value section
    content.push(Line::from(Span::styled(
        "Value",
        Style::default().add_modifier(Modifier::BOLD),
    )));
    let divider = "\u{2500}".repeat(width.min(30));
    content.push(Line::from(Span::styled(
        divider.clone(),
        Style::default().fg(DIM_GRAY),
    )));

    match node {
        ConfigNode::Leaf(val) => {
            let color = value_color(val);
            for line in format_value_full(val) {
                content.push(Line::from(Span::styled(line, Style::default().fg(color))));
            }
        }
        ConfigNode::Branch(children) => {
            content.push(Line::from(Span::styled(
                format!("{{ ... }} ({} keys)", children.len()),
                Style::default().fg(DIM_GRAY),
            )));
        }
    }

    content.push(Line::from(""));

    // Dependencies
    let deps = deps_index.dependencies.get(&path_str);
    let dep_items: Vec<&String> = deps.map(|v| v.iter().collect()).unwrap_or_default();

    content.push(Line::from(Span::styled(
        format!("Dependencies ({})", dep_items.len()),
        Style::default().add_modifier(Modifier::BOLD),
    )));
    content.push(Line::from(Span::styled(
        divider.clone(),
        Style::default().fg(DIM_GRAY),
    )));

    if dep_items.is_empty() {
        content.push(Line::from(Span::styled(
            "  (none)",
            Style::default().fg(DIM_GRAY),
        )));
    } else {
        for (idx, dep) in dep_items.iter().enumerate() {
            let dep_section = dep.split('.').next().unwrap_or("");
            let dep_color = section_color(dep_section);
            let is_selected = focus_detail && idx == detail_cursor;
            let bg = if is_selected { CURSOR_BG } else { Color::Reset };
            let prefix = if is_selected { "> " } else { "  " };
            content.push(Line::from(vec![
                Span::styled(prefix, Style::default().bg(bg)),
                Span::styled(
                    dep.to_string(),
                    Style::default()
                        .fg(dep_color)
                        .bg(bg)
                        .add_modifier(if is_selected {
                            Modifier::BOLD
                        } else {
                            Modifier::empty()
                        }),
                ),
            ]));
        }
    }

    content.push(Line::from(""));

    // Dependents
    let rev_deps = deps_index.dependents.get(&path_str);
    let rev_items: Vec<&String> = rev_deps.map(|v| v.iter().collect()).unwrap_or_default();

    let dep_count = dep_items.len();
    content.push(Line::from(Span::styled(
        format!("Dependents ({})", rev_items.len()),
        Style::default().add_modifier(Modifier::BOLD),
    )));
    content.push(Line::from(Span::styled(
        divider,
        Style::default().fg(DIM_GRAY),
    )));

    if rev_items.is_empty() {
        content.push(Line::from(Span::styled(
            "  (none)",
            Style::default().fg(DIM_GRAY),
        )));
    } else {
        for (idx, dep) in rev_items.iter().enumerate() {
            let dep_section = dep.split('.').next().unwrap_or("");
            let dep_color = section_color(dep_section);
            let global_idx = dep_count + idx;
            let is_selected = focus_detail && global_idx == detail_cursor;
            let bg = if is_selected { CURSOR_BG } else { Color::Reset };
            let prefix = if is_selected { "> " } else { "  " };
            content.push(Line::from(vec![
                Span::styled(prefix, Style::default().bg(bg)),
                Span::styled(
                    dep.to_string(),
                    Style::default()
                        .fg(dep_color)
                        .bg(bg)
                        .add_modifier(if is_selected {
                            Modifier::BOLD
                        } else {
                            Modifier::empty()
                        }),
                ),
            ]));
        }
    }

    // Apply scroll
    let total_lines = content.len();
    let start = right_scroll.min(total_lines);
    let end = total_lines.min(start + visible_height);
    let mut result: Vec<Line<'a>> = content.into_iter().skip(start).take(end - start).collect();
    while result.len() < visible_height {
        result.push(Line::from(""));
    }
    result
}

/// Count total detail items (deps + dependents) for cursor clamping
fn detail_item_count(path_str: &str, deps_index: &DepsIndex) -> usize {
    let dep_count = deps_index
        .dependencies
        .get(path_str)
        .map(|v| v.len())
        .unwrap_or(0);
    let rev_count = deps_index
        .dependents
        .get(path_str)
        .map(|v| v.len())
        .unwrap_or(0);
    dep_count + rev_count
}

/// Get the dep path at a given detail cursor position
fn detail_item_at(path_str: &str, deps_index: &DepsIndex, cursor: usize) -> Option<Vec<String>> {
    let deps: Vec<&String> = deps_index
        .dependencies
        .get(path_str)
        .map(|v| v.iter().collect())
        .unwrap_or_default();
    let revs: Vec<&String> = deps_index
        .dependents
        .get(path_str)
        .map(|v| v.iter().collect())
        .unwrap_or_default();

    if cursor < deps.len() {
        Some(deps[cursor].split('.').map(|s| s.to_string()).collect())
    } else {
        let rev_idx = cursor - deps.len();
        revs.get(rev_idx)
            .map(|s| s.split('.').map(|p| p.to_string()).collect())
    }
}

// ---------------------------------------------------------------------------
// Non-TTY text output
// ---------------------------------------------------------------------------

fn print_tree_text(children: &[(String, ConfigNode)], depth: usize) {
    for (name, node) in children {
        let indent = "  ".repeat(depth);
        match node {
            ConfigNode::Branch(ch) => {
                println!("{}{}/", indent, name);
                print_tree_text(ch, depth + 1);
            }
            ConfigNode::Leaf(val) => {
                println!("{}{} = {}", indent, name, format_value_short(val));
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Main entry
// ---------------------------------------------------------------------------

pub fn run(config: &str, explicit: bool) -> Result<()> {
    let json = resolve::resolve(config, explicit)?;

    let root_children: Vec<(String, ConfigNode)> = match &json {
        Value::Object(map) => {
            let mut children: Vec<(String, ConfigNode)> = map
                .iter()
                .map(|(k, v)| (k.clone(), build_config_tree(v)))
                .collect();
            children.sort_by(|(a, _), (b, _)| a.cmp(b));
            children
        }
        _ => vec![("config".to_string(), build_config_tree(&json))],
    };

    if !tui::is_tty() {
        print_tree_text(&root_children, 0);
        return Ok(());
    }

    // Load deps (graceful fallback)
    let deps_index = match resolve::resolve_deps(config) {
        Ok(deps_json) => build_deps_index(&deps_json),
        Err(_) => DepsIndex {
            dependencies: HashMap::new(),
            dependents: HashMap::new(),
        },
    };

    let mut state = MillerState {
        path: Vec::new(),
        cursor: 0,
        scroll: 0,
        cursor_stack: Vec::new(),
        right_scroll: 0,
        detail_cursor: 0,
        focus: Focus::Middle,
        force_detail: false,
    };

    let mut mode = Mode::Normal;
    let mut search_query = String::new();
    let mut search_results: Vec<Vec<String>> = Vec::new();
    let mut search_cursor: usize = 0;
    let mut search_scroll: usize = 0;
    let mut status_msg: Option<String> = None;

    let mut terminal = tui::setup()?;

    loop {
        // Get current children for middle pane
        let middle_children = get_children_at_path(&root_children, &state.path);
        let middle_count = middle_children.map(|c| c.len()).unwrap_or(0);

        // Clamp middle cursor
        if middle_count == 0 {
            state.cursor = 0;
        } else if state.cursor >= middle_count {
            state.cursor = middle_count - 1;
        }

        terminal.draw(|frame| {
            let size = frame.area();
            let outer = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(1),
                    Constraint::Min(1),
                    Constraint::Length(1),
                ])
                .split(size);

            // ---- Header ----
            let mut header_spans: Vec<Span> = vec![Span::styled(
                format!(" {} ", config),
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            )];

            if !state.path.is_empty() {
                header_spans.push(Span::styled(" ", Style::default()));
                for (i, seg) in state.path.iter().enumerate() {
                    let seg_section = if i == 0 {
                        seg.as_str()
                    } else {
                        state.path[0].as_str()
                    };
                    if i > 0 {
                        header_spans
                            .push(Span::styled(".", Style::default().fg(DIM_GRAY)));
                    }
                    header_spans.push(Span::styled(
                        seg.clone(),
                        Style::default().fg(section_color(seg_section)),
                    ));
                }
            }

            frame.render_widget(
                Paragraph::new(Line::from(header_spans))
                    .style(Style::default().bg(Color::Rgb(20, 20, 30))),
                outer[0],
            );

            // ---- Body: 3 panes ----
            let body = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Percentage(25),
                    Constraint::Percentage(35),
                    Constraint::Percentage(40),
                ])
                .split(outer[1]);

            let is_search_results = matches!(mode, Mode::SearchResults);

            // -- Left pane --
            let left_block = make_block("Parent", false);
            let left_inner = left_block.inner(body[0]);
            let left_height = left_inner.height as usize;
            let left_width = left_inner.width;

            let left_lines = if state.path.is_empty() {
                vec![Line::from(""); left_height]
            } else {
                let parent_path = &state.path[..state.path.len() - 1];
                if let Some(parent_children) = get_children_at_path(&root_children, parent_path) {
                    let highlight = state.path.last().map(|s| s.as_str());
                    // Compute scroll for left pane to show highlighted item
                    let highlight_idx = highlight.and_then(|h| {
                        parent_children.iter().position(|(n, _)| n == h)
                    });
                    let left_scroll = highlight_idx
                        .map(|idx| idx.saturating_sub(left_height / 2))
                        .unwrap_or(0);
                    render_pane_list(
                        parent_children,
                        parent_path,
                        highlight,
                        None,
                        left_scroll,
                        left_height,
                        left_width,
                    )
                } else {
                    vec![Line::from(""); left_height]
                }
            };

            frame.render_widget(left_block, body[0]);
            frame.render_widget(Paragraph::new(left_lines), left_inner);

            // -- Middle pane --
            let middle_active = state.focus == Focus::Middle;
            let middle_title = if is_search_results {
                "Search Results"
            } else {
                "Browse"
            };
            let middle_block = make_block(middle_title, middle_active);
            let middle_inner = middle_block.inner(body[1]);
            let middle_height = middle_inner.height as usize;
            let middle_width = middle_inner.width;

            let middle_lines = if is_search_results {
                // Render search results
                let total = search_results.len();
                clamp_cursor(&mut search_cursor, &mut search_scroll, total, middle_height);
                let end = total.min(search_scroll + middle_height);
                let mut lines = Vec::new();
                for i in search_scroll..end {
                    let result_path = &search_results[i];
                    let section = result_path.first().map(|s| s.as_str()).unwrap_or("");
                    let color = section_color(section);
                    let display = result_path.join(".");
                    let is_selected = i == search_cursor;
                    let bg = if is_selected { CURSOR_BG } else { Color::Reset };
                    lines.push(Line::from(Span::styled(
                        display,
                        Style::default().fg(color).bg(bg).add_modifier(
                            if is_selected {
                                Modifier::BOLD
                            } else {
                                Modifier::empty()
                            },
                        ),
                    )));
                }
                while lines.len() < middle_height {
                    lines.push(Line::from(""));
                }
                lines
            } else if let Some(children) = middle_children {
                clamp_cursor(&mut state.cursor, &mut state.scroll, middle_count, middle_height);
                render_pane_list(
                    children,
                    &state.path,
                    None,
                    Some(state.cursor),
                    state.scroll,
                    middle_height,
                    middle_width,
                )
            } else {
                vec![Line::from(""); middle_height]
            };

            frame.render_widget(middle_block, body[1]);
            frame.render_widget(Paragraph::new(middle_lines), middle_inner);

            // -- Right pane --
            // Determine what's selected
            let selected: Option<(&str, &ConfigNode)> = if is_search_results {
                if !search_results.is_empty() {
                    let result_path = &search_results[search_cursor];
                    if result_path.len() >= 1 {
                        let parent = &result_path[..result_path.len() - 1];
                        let name = &result_path[result_path.len() - 1];
                        get_node_at_path(&root_children, parent, name)
                            .map(|n| (name.as_str(), n))
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                middle_children.and_then(|ch| {
                    ch.get(state.cursor).map(|(n, node)| (n.as_str(), node))
                })
            };

            let show_detail = match selected {
                Some((_, ConfigNode::Leaf(_))) => true,
                Some((_, ConfigNode::Branch(_))) => state.force_detail,
                _ => false,
            };

            let right_title = if show_detail { "Detail" } else { "Preview" };
            let right_active = state.focus == Focus::Detail;
            let right_block = make_block(right_title, right_active || show_detail);
            let right_inner = right_block.inner(body[2]);
            let right_height = right_inner.height as usize;
            let right_width = right_inner.width;

            let right_lines = if let Some((name, node)) = selected {
                if show_detail {
                    let mut full_path = if is_search_results && !search_results.is_empty() {
                        search_results[search_cursor].clone()
                    } else {
                        state.path.clone()
                    };
                    if !is_search_results {
                        full_path.push(name.to_string());
                    }
                    render_detail_pane(
                        &full_path,
                        node,
                        &deps_index,
                        state.detail_cursor,
                        state.right_scroll,
                        state.focus == Focus::Detail,
                        right_width,
                        right_height,
                    )
                } else if let ConfigNode::Branch(children) = node {
                    // Preview children
                    let preview_path = {
                        let mut p = state.path.clone();
                        p.push(name.to_string());
                        p
                    };
                    render_pane_list(
                        children,
                        &preview_path,
                        None,
                        None,
                        state.right_scroll,
                        right_height,
                        right_width,
                    )
                } else {
                    vec![Line::from(""); right_height]
                }
            } else {
                vec![Line::from(""); right_height]
            };

            frame.render_widget(right_block, body[2]);
            frame.render_widget(Paragraph::new(right_lines), right_inner);

            // ---- Footer ----
            let footer_line = match &mode {
                Mode::Normal => {
                    if let Some(msg) = &status_msg {
                        Line::from(Span::styled(
                            format!(" {} ", msg),
                            Style::default().fg(Color::Rgb(0x59, 0xa1, 0x4f)),
                        ))
                    } else {
                        let pos = format!("[{}/{}]", state.cursor + 1, middle_count);
                        Line::from(vec![
                            Span::styled(" j/k", Style::default().add_modifier(Modifier::BOLD)),
                            Span::raw(":move "),
                            Span::styled("l/h", Style::default().add_modifier(Modifier::BOLD)),
                            Span::raw(":in/out "),
                            Span::styled("Tab", Style::default().add_modifier(Modifier::BOLD)),
                            Span::raw(":detail "),
                            Span::styled("/", Style::default().add_modifier(Modifier::BOLD)),
                            Span::raw(":search "),
                            Span::styled("d", Style::default().add_modifier(Modifier::BOLD)),
                            Span::raw(":deps "),
                            Span::styled("q", Style::default().add_modifier(Modifier::BOLD)),
                            Span::raw(":quit "),
                            Span::styled(pos, Style::default().fg(DIM_GRAY)),
                        ])
                    }
                }
                Mode::Search => Line::from(vec![
                    Span::styled(" /", Style::default().add_modifier(Modifier::BOLD)),
                    Span::raw(&search_query),
                    Span::styled("\u{2588}", Style::default().fg(Color::White)),
                ]),
                Mode::SearchResults => {
                    let total = search_results.len();
                    let pos = if total > 0 {
                        format!("[{}/{}]", search_cursor + 1, total)
                    } else {
                        "[0/0]".to_string()
                    };
                    Line::from(vec![
                        Span::styled(
                            format!(" \"{}\" ", search_query),
                            Style::default().fg(Color::Rgb(0xed, 0xc9, 0x48)),
                        ),
                        Span::styled("Enter", Style::default().add_modifier(Modifier::BOLD)),
                        Span::raw(":jump "),
                        Span::styled("Esc", Style::default().add_modifier(Modifier::BOLD)),
                        Span::raw(":back "),
                        Span::styled(pos, Style::default().fg(DIM_GRAY)),
                    ])
                }
            };
            frame.render_widget(
                Paragraph::new(footer_line).style(Style::default().bg(Color::Rgb(20, 20, 30))),
                outer[2],
            );
        })?;

        let key = tui::read_key()?;

        match &mode {
            Mode::Normal => {
                status_msg = None;
                if tui::is_quit(&key) {
                    break;
                }

                match state.focus {
                    Focus::Middle => match key.code {
                        KeyCode::Char('j') | KeyCode::Down => {
                            if middle_count > 0 && state.cursor + 1 < middle_count {
                                state.cursor += 1;
                                state.right_scroll = 0;
                                state.detail_cursor = 0;
                                state.force_detail = false;
                            }
                        }
                        KeyCode::Char('k') | KeyCode::Up => {
                            if state.cursor > 0 {
                                state.cursor -= 1;
                                state.right_scroll = 0;
                                state.detail_cursor = 0;
                                state.force_detail = false;
                            }
                        }
                        KeyCode::Char('g') | KeyCode::Home => {
                            state.cursor = 0;
                            state.right_scroll = 0;
                            state.detail_cursor = 0;
                            state.force_detail = false;
                        }
                        KeyCode::Char('G') | KeyCode::End => {
                            state.cursor = middle_count.saturating_sub(1);
                            state.right_scroll = 0;
                            state.detail_cursor = 0;
                            state.force_detail = false;
                        }
                        KeyCode::PageDown => {
                            let visible =
                                terminal.size()?.height.saturating_sub(4) as usize;
                            state.cursor =
                                (state.cursor + visible).min(middle_count.saturating_sub(1));
                            state.right_scroll = 0;
                            state.detail_cursor = 0;
                        }
                        KeyCode::PageUp => {
                            let visible =
                                terminal.size()?.height.saturating_sub(4) as usize;
                            state.cursor = state.cursor.saturating_sub(visible);
                            state.right_scroll = 0;
                            state.detail_cursor = 0;
                        }
                        KeyCode::Char('l') | KeyCode::Right | KeyCode::Enter => {
                            // Drill in
                            if let Some(children) =
                                get_children_at_path(&root_children, &state.path)
                            {
                                if let Some((name, node)) = children.get(state.cursor) {
                                    if matches!(node, ConfigNode::Branch(_)) {
                                        state
                                            .cursor_stack
                                            .push((state.cursor, state.scroll));
                                        state.path.push(name.clone());
                                        state.cursor = 0;
                                        state.scroll = 0;
                                        state.right_scroll = 0;
                                        state.detail_cursor = 0;
                                        state.force_detail = false;
                                    }
                                }
                            }
                        }
                        KeyCode::Char('h') | KeyCode::Left => {
                            // Go back
                            if !state.path.is_empty() {
                                state.path.pop();
                                if let Some((c, s)) = state.cursor_stack.pop() {
                                    state.cursor = c;
                                    state.scroll = s;
                                } else {
                                    state.cursor = 0;
                                    state.scroll = 0;
                                }
                                state.right_scroll = 0;
                                state.detail_cursor = 0;
                                state.force_detail = false;
                            }
                        }
                        KeyCode::Tab => {
                            state.force_detail = !state.force_detail;
                        }
                        KeyCode::Char('d') => {
                            // Enter detail focus
                            state.focus = Focus::Detail;
                            state.detail_cursor = 0;
                        }
                        KeyCode::Char('J') => {
                            state.right_scroll += 1;
                        }
                        KeyCode::Char('K') => {
                            state.right_scroll = state.right_scroll.saturating_sub(1);
                        }
                        KeyCode::Char('/') => {
                            mode = Mode::Search;
                            search_query.clear();
                        }
                        _ => {}
                    },
                    Focus::Detail => match key.code {
                        KeyCode::Char('j') | KeyCode::Down => {
                            let mut full_path = state.path.clone();
                            if let Some(children) =
                                get_children_at_path(&root_children, &state.path)
                            {
                                if let Some((name, _)) = children.get(state.cursor) {
                                    full_path.push(name.clone());
                                }
                            }
                            let path_str = full_path.join(".");
                            let total = detail_item_count(&path_str, &deps_index);
                            if total > 0 && state.detail_cursor + 1 < total {
                                state.detail_cursor += 1;
                            }
                        }
                        KeyCode::Char('k') | KeyCode::Up => {
                            state.detail_cursor = state.detail_cursor.saturating_sub(1);
                        }
                        KeyCode::Enter => {
                            // Jump to selected dependency
                            let mut full_path = state.path.clone();
                            if let Some(children) =
                                get_children_at_path(&root_children, &state.path)
                            {
                                if let Some((name, _)) = children.get(state.cursor) {
                                    full_path.push(name.clone());
                                }
                            }
                            let path_str = full_path.join(".");
                            if let Some(target) =
                                detail_item_at(&path_str, &deps_index, state.detail_cursor)
                            {
                                jump_to_path(&mut state, &target, &root_children);
                                state.force_detail = true;
                                status_msg = Some(format!("Jumped to {}", target.join(".")));
                            }
                        }
                        KeyCode::Esc | KeyCode::Char('h') | KeyCode::Left => {
                            state.focus = Focus::Middle;
                        }
                        KeyCode::Char('q') => break,
                        _ => {}
                    },
                }
            }
            Mode::Search => match key.code {
                KeyCode::Enter => {
                    if search_query.is_empty() {
                        mode = Mode::Normal;
                    } else {
                        search_results = search_tree(&root_children, &search_query, &[]);
                        search_cursor = 0;
                        search_scroll = 0;
                        if search_results.is_empty() {
                            mode = Mode::Normal;
                            status_msg = Some(format!("No matches for '{}'", search_query));
                        } else {
                            mode = Mode::SearchResults;
                        }
                    }
                }
                KeyCode::Esc => {
                    mode = Mode::Normal;
                }
                KeyCode::Backspace => {
                    search_query.pop();
                }
                KeyCode::Char(c) => {
                    search_query.push(c);
                }
                _ => {}
            },
            Mode::SearchResults => match key.code {
                KeyCode::Char('j') | KeyCode::Down => {
                    if !search_results.is_empty() && search_cursor + 1 < search_results.len() {
                        search_cursor += 1;
                    }
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    search_cursor = search_cursor.saturating_sub(1);
                }
                KeyCode::Enter => {
                    if !search_results.is_empty() {
                        let target = search_results[search_cursor].clone();
                        jump_to_path(&mut state, &target, &root_children);
                        mode = Mode::Normal;
                        status_msg = Some(format!("Jumped to {}", target.join(".")));
                    }
                }
                KeyCode::Esc | KeyCode::Char('q') => {
                    mode = Mode::Normal;
                }
                KeyCode::Char('/') => {
                    mode = Mode::Search;
                    search_query.clear();
                }
                _ => {}
            },
        }
    }

    tui::teardown(terminal)?;
    Ok(())
}
