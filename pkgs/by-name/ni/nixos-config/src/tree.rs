use std::collections::HashMap;

use anyhow::Result;
use crossterm::event::KeyCode;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph};
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
    Deps,
}

struct MillerState {
    path: Vec<String>,
    cursor: usize,
    scroll: usize,
    path_memory: HashMap<Vec<String>, (usize, usize)>,
    detail_scroll: usize,
    deps_cursor: usize,
    focus: Focus,
}

enum Mode {
    Normal,
    Search {
        query: String,
        results: Vec<Vec<String>>,
        cursor: usize,
        scroll: usize,
    },
    Help,
}

// ---------------------------------------------------------------------------
// OneHalfDark UI palette (text colors only, no bg changes)
// ---------------------------------------------------------------------------

/// OneHalfDark foreground
const FG: Color = Color::Rgb(0xdc, 0xdf, 0xe4);
/// OneHalfDark comment / dim text
const COMMENT: Color = Color::Rgb(0x5c, 0x63, 0x70);
/// OneHalfDark gutter / subtle bg accents
const GUTTER: Color = Color::Rgb(0x4b, 0x52, 0x63);
/// OneHalfDark blue (active borders, accents)
const BLUE: Color = Color::Rgb(0x61, 0xaf, 0xef);
/// OneHalfDark magenta (footer desc bg)
const MAGENTA: Color = Color::Rgb(0xc6, 0x78, 0xdd);
/// OneHalfDark green (status messages)
const GREEN: Color = Color::Rgb(0x98, 0xc3, 0x79);
/// OneHalfDark red
const RED: Color = Color::Rgb(0xe0, 0x6c, 0x75);
/// OneHalfDark yellow
const YELLOW: Color = Color::Rgb(0xe5, 0xc0, 0x7b);
/// OneHalfDark cyan
const CYAN: Color = Color::Rgb(0x56, 0xb6, 0xc2);

// UI element colors derived from the palette
const ACTIVE_BORDER: Color = BLUE;
const INACTIVE_BORDER: Color = GUTTER;
const CURSOR_BG: Color = Color::Rgb(0x2c, 0x31, 0x3c);
const PARENT_HIGHLIGHT_BG: Color = Color::Rgb(0x23, 0x27, 0x30);
const HIGHLIGHT_BG: Color = Color::Rgb(0x80, 0x60, 0x00);
const HEADER_BG: Color = Color::Rgb(0x21, 0x25, 0x2b);
/// Footer key pill background
const KEY_BG: Color = Color::Rgb(0x3e, 0x44, 0x52);
/// Footer key pill foreground
const KEY_FG: Color = FG;
/// Footer desc pill background
const DESC_BG: Color = MAGENTA;
/// Footer desc pill foreground
const DESC_FG: Color = Color::Rgb(0x21, 0x25, 0x2b);

// ---------------------------------------------------------------------------
// Nerd font icons & their colors (independent of section colors)
// ---------------------------------------------------------------------------

/// Folder icon for branches (nf-fa-folder U+F07B)
const ICON_BRANCH: &str = "\u{f07b}";
const ICON_BRANCH_COLOR: Color = YELLOW;

/// Leaf icons by value type
const ICON_BOOL: &str = "\u{f205}";      // nf-fa-toggle_on
const ICON_BOOL_COLOR: Color = CYAN;
const ICON_STRING: &str = "\u{f10d}";    // nf-fa-quote_left
const ICON_STRING_COLOR: Color = GREEN;
const ICON_NUMBER: &str = "\u{f292}";    // nf-fa-hashtag
const ICON_NUMBER_COLOR: Color = MAGENTA;
const ICON_NULL: &str = "\u{f071}";      // nf-fa-exclamation_triangle
const ICON_NULL_COLOR: Color = COMMENT;
const ICON_ARRAY: &str = "\u{f03a}";     // nf-fa-list
const ICON_ARRAY_COLOR: Color = YELLOW;
const ICON_OBJECT: &str = "\u{f1b2}";    // nf-fa-cube
const ICON_OBJECT_COLOR: Color = COMMENT;

fn node_icon(node: &ConfigNode) -> (&'static str, Color) {
    match node {
        ConfigNode::Branch(_) => (ICON_BRANCH, ICON_BRANCH_COLOR),
        ConfigNode::Leaf(val) => match val {
            Value::Bool(_) => (ICON_BOOL, ICON_BOOL_COLOR),
            Value::String(_) => (ICON_STRING, ICON_STRING_COLOR),
            Value::Number(_) => (ICON_NUMBER, ICON_NUMBER_COLOR),
            Value::Null => (ICON_NULL, ICON_NULL_COLOR),
            Value::Array(_) => (ICON_ARRAY, ICON_ARRAY_COLOR),
            Value::Object(_) => (ICON_OBJECT, ICON_OBJECT_COLOR),
        },
    }
}

// ---------------------------------------------------------------------------
// Section colors (for option names — unchanged)
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
        Value::Bool(true) => GREEN,
        Value::Bool(false) => RED,
        Value::Number(_) => CYAN,
        Value::String(_) => YELLOW,
        Value::Null => COMMENT,
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
                dependents.entry(accessed).or_default().push(accessor);
            }
        }
    }

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
        Value::Array(arr) => format!("[{}]", arr.len()),
        Value::Object(map) if map.is_empty() => "{ }".to_string(),
        Value::Object(map) => format!("{{{}}}", map.len()),
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
    if query.is_empty() {
        return results;
    }
    let lower_query = query.to_lowercase();
    for (name, node) in root {
        let mut full_path = prefix.to_vec();
        full_path.push(name.clone());
        let joined = full_path.join(".");
        if joined.to_lowercase().contains(&lower_query) {
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

    state
        .path_memory
        .insert(state.path.clone(), (state.cursor, state.scroll));

    state.path.clear();
    state.scroll = 0;
    state.detail_scroll = 0;
    state.deps_cursor = 0;

    let parent = &target_path[..target_path.len() - 1];
    let leaf = &target_path[target_path.len() - 1];

    let mut current = root;
    for segment in parent {
        if let Some(idx) = current.iter().position(|(n, _)| n == segment) {
            state.path_memory.insert(state.path.clone(), (idx, 0));
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

fn make_block(title: &str, active: bool) -> Block<'_> {
    let border_color = if active { ACTIVE_BORDER } else { INACTIVE_BORDER };
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_color))
        .title(Span::styled(
            format!(" {} ", title),
            Style::default()
                .fg(border_color)
                .add_modifier(Modifier::BOLD),
        ))
}

/// Render a footer "pill": ` key `` desc ` with contrasting backgrounds.
fn footer_pill<'a>(key: &str, desc: &str) -> Vec<Span<'a>> {
    vec![
        Span::styled(
            format!(" {} ", key),
            Style::default()
                .fg(KEY_FG)
                .bg(KEY_BG)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(" {} ", desc),
            Style::default().fg(DESC_FG).bg(DESC_BG),
        ),
        Span::raw(" "),
    ]
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
    if width < 4 {
        return vec![Line::from(""); visible_height];
    }
    let end = children.len().min(scroll + visible_height);
    let mut lines = Vec::new();

    // Icon column takes 2 chars: icon + space
    let icon_col = 2;
    let text_width = width.saturating_sub(icon_col);

    for i in scroll..end {
        let (name, node) = &children[i];
        let section = top_level_section(state_path, name);
        let color = section_color(&section);
        let (icon, icon_color) = node_icon(node);

        let is_cursor = cursor_idx == Some(i);
        let is_highlight = highlight_name == Some(name.as_str());

        let bg = if is_cursor {
            CURSOR_BG
        } else if is_highlight {
            PARENT_HIGHLIGHT_BG
        } else {
            Color::Reset
        };

        let name_mod = if is_cursor || is_highlight {
            Modifier::BOLD
        } else {
            Modifier::empty()
        };

        let name_style = Style::default().fg(color).bg(bg).add_modifier(name_mod);

        let suffix = match node {
            ConfigNode::Branch(ch) => {
                let count_str = format!("{}", ch.len());
                (count_str, Style::default().fg(COMMENT).bg(bg))
            }
            ConfigNode::Leaf(val) => {
                let short = format_value_short(val);
                let max_suffix = text_width / 2;
                let short = if max_suffix > 3 && short.len() > max_suffix {
                    format!("{}...", &short[..max_suffix - 3])
                } else {
                    short
                };
                (short, Style::default().fg(value_color(val)).bg(bg))
            }
        };

        let name_display = if name.len() + 1 + suffix.0.len() > text_width && name.len() > 3 {
            let max_name = text_width.saturating_sub(suffix.0.len() + 4);
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
        let padding = if name_len + suffix_len + 1 < text_width {
            text_width - name_len - suffix_len
        } else {
            1
        };

        lines.push(Line::from(vec![
            Span::styled(
                format!("{} ", icon),
                Style::default().fg(icon_color).bg(bg),
            ),
            Span::styled(name_display, name_style),
            Span::styled(" ".repeat(padding), Style::default().bg(bg)),
            Span::styled(suffix.0, suffix.1),
        ]));
    }

    for _ in lines.len()..visible_height {
        lines.push(Line::from(""));
    }

    lines
}

/// Render a search result line with match highlighting.
fn render_search_result_line<'a>(
    path: &[String],
    query: &str,
    is_selected: bool,
    _width: u16,
) -> Line<'a> {
    let section = path.first().map(|s| s.as_str()).unwrap_or("");
    let color = section_color(section);
    let bg = if is_selected { CURSOR_BG } else { Color::Reset };
    let bold = if is_selected {
        Modifier::BOLD
    } else {
        Modifier::empty()
    };

    let display = path.join(".");
    let lower_display = display.to_lowercase();
    let lower_query = query.to_lowercase();

    if !lower_query.is_empty() {
        if let Some(start) = lower_display.find(&lower_query) {
            let end = start + query.len();
            let mut spans = Vec::new();
            if start > 0 {
                spans.push(Span::styled(
                    display[..start].to_string(),
                    Style::default().fg(color).bg(bg).add_modifier(bold),
                ));
            }
            spans.push(Span::styled(
                display[start..end].to_string(),
                Style::default()
                    .fg(Color::White)
                    .bg(HIGHLIGHT_BG)
                    .add_modifier(Modifier::BOLD),
            ));
            if end < display.len() {
                spans.push(Span::styled(
                    display[end..].to_string(),
                    Style::default().fg(color).bg(bg).add_modifier(bold),
                ));
            }
            return Line::from(spans);
        }
    }

    Line::from(Span::styled(
        display,
        Style::default().fg(color).bg(bg).add_modifier(bold),
    ))
}

// ---------------------------------------------------------------------------
// Bottom pane renderers
// ---------------------------------------------------------------------------

fn render_detail_info<'a>(
    full_path: &[String],
    node: &ConfigNode,
    scroll: usize,
    inner_width: u16,
    visible_height: usize,
) -> Vec<Line<'a>> {
    let width = inner_width as usize;
    let section = if full_path.is_empty() {
        ""
    } else {
        &full_path[0]
    };
    let path_color = section_color(section);

    let mut content: Vec<Line<'a>> = Vec::new();

    content.push(Line::from(Span::styled(
        full_path.join("."),
        Style::default()
            .fg(path_color)
            .add_modifier(Modifier::BOLD),
    )));
    content.push(Line::from(""));

    let divider = "\u{2500}".repeat(width.min(30));
    content.push(Line::from(Span::styled(
        "Value",
        Style::default().fg(FG).add_modifier(Modifier::BOLD),
    )));
    content.push(Line::from(Span::styled(
        divider,
        Style::default().fg(COMMENT),
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
                format!("{} children", children.len()),
                Style::default().fg(COMMENT),
            )));
        }
    }

    let total = content.len();
    let start = scroll.min(total);
    let end = total.min(start + visible_height);
    let mut result: Vec<Line<'a>> = content.into_iter().skip(start).take(end - start).collect();
    while result.len() < visible_height {
        result.push(Line::from(""));
    }
    result
}

fn render_dep_list<'a>(
    items: &[String],
    cursor: Option<usize>,
    scroll: usize,
    visible_height: usize,
) -> Vec<Line<'a>> {
    if items.is_empty() {
        let mut lines = vec![Line::from(Span::styled(
            "  (none)",
            Style::default().fg(COMMENT),
        ))];
        while lines.len() < visible_height {
            lines.push(Line::from(""));
        }
        return lines;
    }

    let end = items.len().min(scroll + visible_height);
    let mut lines = Vec::new();
    for i in scroll..end {
        let dep = &items[i];
        let dep_section = dep.split('.').next().unwrap_or("");
        let dep_color = section_color(dep_section);
        let is_selected = cursor == Some(i);
        let bg = if is_selected { CURSOR_BG } else { Color::Reset };
        let prefix = if is_selected { "> " } else { "  " };
        lines.push(Line::from(vec![
            Span::styled(prefix, Style::default().fg(FG).bg(bg)),
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

    while lines.len() < visible_height {
        lines.push(Line::from(""));
    }
    lines
}

/// Combined detail for search mode right pane.
fn render_search_detail<'a>(
    full_path: &[String],
    node: &ConfigNode,
    deps_index: &DepsIndex,
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
    let divider = "\u{2500}".repeat(width.min(30));

    let mut content: Vec<Line<'a>> = Vec::new();

    content.push(Line::from(Span::styled(
        path_str.clone(),
        Style::default()
            .fg(path_color)
            .add_modifier(Modifier::BOLD),
    )));
    content.push(Line::from(""));

    content.push(Line::from(Span::styled(
        "Value",
        Style::default().fg(FG).add_modifier(Modifier::BOLD),
    )));
    content.push(Line::from(Span::styled(
        divider.clone(),
        Style::default().fg(COMMENT),
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
                format!("{} children", children.len()),
                Style::default().fg(COMMENT),
            )));
        }
    }

    content.push(Line::from(""));

    let dep_items: Vec<&str> = deps_index
        .dependencies
        .get(&path_str)
        .map(|v| v.iter().map(|s| s.as_str()).collect())
        .unwrap_or_default();
    content.push(Line::from(Span::styled(
        format!("Dependencies ({})", dep_items.len()),
        Style::default().fg(FG).add_modifier(Modifier::BOLD),
    )));
    content.push(Line::from(Span::styled(
        divider.clone(),
        Style::default().fg(COMMENT),
    )));
    if dep_items.is_empty() {
        content.push(Line::from(Span::styled(
            "  (none)",
            Style::default().fg(COMMENT),
        )));
    } else {
        for dep in &dep_items {
            let ds = dep.split('.').next().unwrap_or("");
            content.push(Line::from(Span::styled(
                format!("  {}", dep),
                Style::default().fg(section_color(ds)),
            )));
        }
    }

    content.push(Line::from(""));

    let rev_items: Vec<&str> = deps_index
        .dependents
        .get(&path_str)
        .map(|v| v.iter().map(|s| s.as_str()).collect())
        .unwrap_or_default();
    content.push(Line::from(Span::styled(
        format!("Dependents ({})", rev_items.len()),
        Style::default().fg(FG).add_modifier(Modifier::BOLD),
    )));
    content.push(Line::from(Span::styled(
        divider,
        Style::default().fg(COMMENT),
    )));
    if rev_items.is_empty() {
        content.push(Line::from(Span::styled(
            "  (none)",
            Style::default().fg(COMMENT),
        )));
    } else {
        for dep in &rev_items {
            let ds = dep.split('.').next().unwrap_or("");
            content.push(Line::from(Span::styled(
                format!("  {}", dep),
                Style::default().fg(section_color(ds)),
            )));
        }
    }

    let total = content.len();
    let end = total.min(visible_height);
    let mut result: Vec<Line<'a>> = content.into_iter().take(end).collect();
    while result.len() < visible_height {
        result.push(Line::from(""));
    }
    result
}

fn deps_total_count(path_str: &str, deps_index: &DepsIndex) -> usize {
    let d = deps_index
        .dependencies
        .get(path_str)
        .map(|v| v.len())
        .unwrap_or(0);
    let r = deps_index
        .dependents
        .get(path_str)
        .map(|v| v.len())
        .unwrap_or(0);
    d + r
}

fn dep_item_at(path_str: &str, deps_index: &DepsIndex, cursor: usize) -> Option<Vec<String>> {
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

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let v = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(v[1])[1]
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
// Help text
// ---------------------------------------------------------------------------

const HELP_LINES: &[(&str, &str)] = &[
    ("Navigation", ""),
    ("j / \u{2193}", "Move cursor down"),
    ("k / \u{2191}", "Move cursor up"),
    ("l / \u{2192} / Enter", "Drill into branch"),
    ("h / \u{2190}", "Go back one level"),
    ("g / Home", "Jump to top"),
    ("G / End", "Jump to bottom"),
    ("PageDown / PageUp", "Page scroll"),
    ("", ""),
    ("Info Panes", ""),
    ("J / K", "Scroll detail pane down/up"),
    ("d", "Focus dependencies/dependents"),
    ("j / k (in deps)", "Navigate deps/dependents"),
    ("Enter (in deps)", "Jump to dependency"),
    ("Esc / h (in deps)", "Return to browse pane"),
    ("PageDown/Up (in deps)", "Page through deps"),
    ("", ""),
    ("Search", ""),
    ("/", "Open search"),
    ("\u{2191}\u{2193} (in search)", "Navigate results while typing"),
    ("Enter (in search)", "Jump to selected result"),
    ("Esc (in search)", "Close search"),
    ("PageDown/Up (in search)", "Page through results"),
    ("", ""),
    ("General", ""),
    ("?", "Toggle this help"),
    ("q / Ctrl-C", "Quit"),
];

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
        path_memory: HashMap::new(),
        detail_scroll: 0,
        deps_cursor: 0,
        focus: Focus::Middle,
    };

    let mut mode = Mode::Normal;
    let mut status_msg: Option<String> = None;

    let mut terminal = tui::setup()?;

    loop {
        let middle_children = get_children_at_path(&root_children, &state.path);
        let middle_count = middle_children.map(|c| c.len()).unwrap_or(0);

        if middle_count == 0 {
            state.cursor = 0;
        } else if state.cursor >= middle_count {
            state.cursor = middle_count - 1;
        }

        terminal.draw(|frame| {
            let size = frame.area();
            let screen_width = size.width;

            match &mode {
                // =============================================================
                // Search layout
                // =============================================================
                Mode::Search {
                    query,
                    results,
                    cursor: s_cursor,
                    scroll: s_scroll,
                } => {
                    let outer = Layout::default()
                        .direction(Direction::Vertical)
                        .constraints([
                            Constraint::Length(1),
                            Constraint::Min(1),
                            Constraint::Length(3),
                            Constraint::Length(1),
                        ])
                        .split(size);

                    // Header
                    let total = results.len();
                    let header = Line::from(vec![
                        Span::styled(
                            " Search ",
                            Style::default().fg(FG).add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(
                            format!(" {} results", total),
                            Style::default().fg(COMMENT),
                        ),
                    ]);
                    frame.render_widget(
                        Paragraph::new(header).style(Style::default().bg(HEADER_BG)),
                        outer[0],
                    );

                    // Body: Results + Detail
                    let body = Layout::default()
                        .direction(Direction::Horizontal)
                        .constraints([
                            Constraint::Percentage(60),
                            Constraint::Percentage(40),
                        ])
                        .split(outer[1]);

                    let results_block = make_block("Results", true);
                    let results_inner = results_block.inner(body[0]);
                    let results_height = results_inner.height as usize;
                    let results_width = results_inner.width;

                    let mut sc = *s_scroll;
                    let mut cu = *s_cursor;
                    clamp_cursor(&mut cu, &mut sc, total, results_height);
                    let end = total.min(sc + results_height);

                    let mut result_lines = Vec::new();
                    for i in sc..end {
                        result_lines.push(render_search_result_line(
                            &results[i],
                            query,
                            i == cu,
                            results_width,
                        ));
                    }
                    while result_lines.len() < results_height {
                        result_lines.push(Line::from(""));
                    }
                    frame.render_widget(results_block, body[0]);
                    frame.render_widget(Paragraph::new(result_lines), results_inner);

                    // Detail
                    let detail_block = make_block("Detail", false);
                    let detail_inner = detail_block.inner(body[1]);
                    let detail_height = detail_inner.height as usize;
                    let detail_width = detail_inner.width;

                    let detail_lines = if !results.is_empty() && cu < results.len() {
                        let rp = &results[cu];
                        let parent = &rp[..rp.len() - 1];
                        let name = &rp[rp.len() - 1];
                        if let Some(node) = get_node_at_path(&root_children, parent, name) {
                            render_search_detail(
                                rp, node, &deps_index, detail_width, detail_height,
                            )
                        } else {
                            vec![Line::from(""); detail_height]
                        }
                    } else {
                        vec![Line::from(""); detail_height]
                    };
                    frame.render_widget(detail_block, body[1]);
                    frame.render_widget(Paragraph::new(detail_lines), detail_inner);

                    // Search bar
                    let search_block = make_block("Search", true);
                    let search_inner = search_block.inner(outer[2]);
                    let search_line = Line::from(vec![
                        Span::styled(query.as_str(), Style::default().fg(FG)),
                        Span::styled("\u{2588}", Style::default().fg(BLUE)),
                    ]);
                    frame.render_widget(search_block, outer[2]);
                    frame.render_widget(Paragraph::new(search_line), search_inner);

                    // Footer
                    let mut footer_spans = vec![Span::raw(" ")];
                    footer_spans.extend(footer_pill("\u{2191}\u{2193}", "select"));
                    footer_spans.extend(footer_pill("Enter", "jump"));
                    footer_spans.extend(footer_pill("Esc", "close"));
                    footer_spans.push(if total > 0 {
                        Span::styled(
                            format!("[{}/{}]", cu + 1, total),
                            Style::default().fg(COMMENT),
                        )
                    } else {
                        Span::styled("[0/0]", Style::default().fg(COMMENT))
                    });
                    frame.render_widget(
                        Paragraph::new(Line::from(footer_spans))
                            .style(Style::default().bg(HEADER_BG)),
                        outer[3],
                    );
                }

                // =============================================================
                // Normal / Help layout
                // =============================================================
                _ => {
                    let outer = Layout::default()
                        .direction(Direction::Vertical)
                        .constraints([
                            Constraint::Length(1),
                            Constraint::Percentage(60),
                            Constraint::Percentage(40),
                            Constraint::Length(1),
                        ])
                        .split(size);

                    // ---- Header ----
                    let mut header_spans: Vec<Span> = vec![Span::styled(
                        format!(" {} ", config),
                        Style::default().fg(FG).add_modifier(Modifier::BOLD),
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
                                header_spans.push(Span::styled(
                                    ".",
                                    Style::default().fg(COMMENT),
                                ));
                            }
                            header_spans.push(Span::styled(
                                seg.clone(),
                                Style::default().fg(section_color(seg_section)),
                            ));
                        }
                    }
                    frame.render_widget(
                        Paragraph::new(Line::from(header_spans))
                            .style(Style::default().bg(HEADER_BG)),
                        outer[0],
                    );

                    // ---- Top row: Parent (25%) | Browse (50%) | Children (25%) ----
                    let top = Layout::default()
                        .direction(Direction::Horizontal)
                        .constraints([
                            Constraint::Percentage(25),
                            Constraint::Percentage(50),
                            Constraint::Percentage(25),
                        ])
                        .split(outer[1]);

                    // Parent
                    let left_block = make_block("Parent", false);
                    let left_inner = left_block.inner(top[0]);
                    let left_height = left_inner.height as usize;
                    let left_width = left_inner.width;

                    let left_lines = if state.path.is_empty() {
                        vec![Line::from(""); left_height]
                    } else {
                        let parent_path = &state.path[..state.path.len() - 1];
                        if let Some(parent_children) =
                            get_children_at_path(&root_children, parent_path)
                        {
                            let highlight = state.path.last().map(|s| s.as_str());
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
                    frame.render_widget(left_block, top[0]);
                    frame.render_widget(Paragraph::new(left_lines), left_inner);

                    // Browse
                    let middle_active = state.focus == Focus::Middle;
                    let middle_block = make_block("Browse", middle_active);
                    let middle_inner = middle_block.inner(top[1]);
                    let middle_height = middle_inner.height as usize;
                    let middle_width = middle_inner.width;

                    let middle_lines = if let Some(children) = middle_children {
                        clamp_cursor(
                            &mut state.cursor,
                            &mut state.scroll,
                            middle_count,
                            middle_height,
                        );
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
                    frame.render_widget(middle_block, top[1]);
                    frame.render_widget(Paragraph::new(middle_lines), middle_inner);

                    // Children
                    let children_block = make_block("Children", false);
                    let children_inner = children_block.inner(top[2]);
                    let children_height = children_inner.height as usize;
                    let children_width = children_inner.width;

                    let selected: Option<(&str, &ConfigNode)> =
                        middle_children.and_then(|ch| {
                            ch.get(state.cursor)
                                .map(|(n, node)| (n.as_str(), node))
                        });

                    let children_lines =
                        if let Some((name, ConfigNode::Branch(ch))) = selected {
                            let preview_path = {
                                let mut p = state.path.clone();
                                p.push(name.to_string());
                                p
                            };
                            render_pane_list(
                                ch,
                                &preview_path,
                                None,
                                None,
                                0,
                                children_height,
                                children_width,
                            )
                        } else {
                            vec![Line::from(""); children_height]
                        };
                    frame.render_widget(children_block, top[2]);
                    frame.render_widget(Paragraph::new(children_lines), children_inner);

                    // ---- Bottom row: Detail | Dependencies | Dependents ----
                    let full_path: Vec<String> = if let Some((name, _)) = selected {
                        let mut p = state.path.clone();
                        p.push(name.to_string());
                        p
                    } else {
                        state.path.clone()
                    };
                    let path_str = full_path.join(".");

                    let dep_items: Vec<String> = deps_index
                        .dependencies
                        .get(&path_str)
                        .cloned()
                        .unwrap_or_default();
                    let rev_items: Vec<String> = deps_index
                        .dependents
                        .get(&path_str)
                        .cloned()
                        .unwrap_or_default();
                    let dep_count = dep_items.len();

                    let deps_focus = state.focus == Focus::Deps;

                    let narrow = screen_width < 110;

                    if narrow {
                        // Narrow: Detail (50%) | stacked Deps+Revs (50%)
                        let bottom = Layout::default()
                            .direction(Direction::Horizontal)
                            .constraints([
                                Constraint::Percentage(50),
                                Constraint::Percentage(50),
                            ])
                            .split(outer[2]);

                        // Detail
                        let detail_block = make_block("Detail", false);
                        let detail_inner = detail_block.inner(bottom[0]);
                        let detail_height = detail_inner.height as usize;
                        let detail_width = detail_inner.width;
                        let detail_lines = if let Some((_, node)) = selected {
                            render_detail_info(
                                &full_path,
                                node,
                                state.detail_scroll,
                                detail_width,
                                detail_height,
                            )
                        } else {
                            vec![Line::from(""); detail_height]
                        };
                        frame.render_widget(detail_block, bottom[0]);
                        frame.render_widget(Paragraph::new(detail_lines), detail_inner);

                        // Stacked deps + dependents
                        let right_stack = Layout::default()
                            .direction(Direction::Vertical)
                            .constraints([
                                Constraint::Percentage(50),
                                Constraint::Percentage(50),
                            ])
                            .split(bottom[1]);

                        // Dependencies
                        let deps_cursor = if deps_focus && state.deps_cursor < dep_count {
                            Some(state.deps_cursor)
                        } else {
                            None
                        };
                        let deps_active = deps_focus && deps_cursor.is_some();
                        let deps_title = format!("Dependencies ({})", dep_count);
                        let deps_block = make_block(&deps_title, deps_active);
                        let deps_inner = deps_block.inner(right_stack[0]);
                        let deps_height = deps_inner.height as usize;
                        let deps_lines =
                            render_dep_list(&dep_items, deps_cursor, 0, deps_height);
                        frame.render_widget(deps_block, right_stack[0]);
                        frame.render_widget(Paragraph::new(deps_lines), deps_inner);

                        // Dependents
                        let rev_cursor = if deps_focus && state.deps_cursor >= dep_count {
                            Some(state.deps_cursor - dep_count)
                        } else {
                            None
                        };
                        let rev_active = deps_focus && rev_cursor.is_some();
                        let rev_title = format!("Dependents ({})", rev_items.len());
                        let rev_block = make_block(&rev_title, rev_active);
                        let rev_inner = rev_block.inner(right_stack[1]);
                        let rev_height = rev_inner.height as usize;
                        let rev_lines =
                            render_dep_list(&rev_items, rev_cursor, 0, rev_height);
                        frame.render_widget(rev_block, right_stack[1]);
                        frame.render_widget(Paragraph::new(rev_lines), rev_inner);
                    } else {
                        // Wide: Detail (50%) | Dependencies (25%) | Dependents (25%)
                        let bottom = Layout::default()
                            .direction(Direction::Horizontal)
                            .constraints([
                                Constraint::Percentage(50),
                                Constraint::Percentage(25),
                                Constraint::Percentage(25),
                            ])
                            .split(outer[2]);

                        // Detail
                        let detail_block = make_block("Detail", false);
                        let detail_inner = detail_block.inner(bottom[0]);
                        let detail_height = detail_inner.height as usize;
                        let detail_width = detail_inner.width;
                        let detail_lines = if let Some((_, node)) = selected {
                            render_detail_info(
                                &full_path,
                                node,
                                state.detail_scroll,
                                detail_width,
                                detail_height,
                            )
                        } else {
                            vec![Line::from(""); detail_height]
                        };
                        frame.render_widget(detail_block, bottom[0]);
                        frame.render_widget(Paragraph::new(detail_lines), detail_inner);

                        // Dependencies
                        let deps_cursor = if deps_focus && state.deps_cursor < dep_count {
                            Some(state.deps_cursor)
                        } else {
                            None
                        };
                        let deps_active = deps_focus && deps_cursor.is_some();
                        let deps_title = format!("Dependencies ({})", dep_count);
                        let deps_block = make_block(&deps_title, deps_active);
                        let deps_inner = deps_block.inner(bottom[1]);
                        let deps_height = deps_inner.height as usize;
                        let deps_lines =
                            render_dep_list(&dep_items, deps_cursor, 0, deps_height);
                        frame.render_widget(deps_block, bottom[1]);
                        frame.render_widget(Paragraph::new(deps_lines), deps_inner);

                        // Dependents
                        let rev_cursor = if deps_focus && state.deps_cursor >= dep_count {
                            Some(state.deps_cursor - dep_count)
                        } else {
                            None
                        };
                        let rev_active = deps_focus && rev_cursor.is_some();
                        let rev_title = format!("Dependents ({})", rev_items.len());
                        let rev_block = make_block(&rev_title, rev_active);
                        let rev_inner = rev_block.inner(bottom[2]);
                        let rev_height = rev_inner.height as usize;
                        let rev_lines =
                            render_dep_list(&rev_items, rev_cursor, 0, rev_height);
                        frame.render_widget(rev_block, bottom[2]);
                        frame.render_widget(Paragraph::new(rev_lines), rev_inner);
                    }

                    // ---- Footer ----
                    let footer_line = if let Some(msg) = &status_msg {
                        Line::from(vec![
                            Span::raw(" "),
                            Span::styled(
                                format!(" {} ", msg),
                                Style::default().fg(DESC_FG).bg(GREEN),
                            ),
                        ])
                    } else {
                        let pos = format!("[{}/{}]", state.cursor + 1, middle_count);
                        let mut spans = vec![Span::raw(" ")];
                        spans.extend(footer_pill("\u{2191}\u{2193}", "move"));
                        spans.extend(footer_pill("\u{2190}\u{2192}", "in/out"));
                        spans.extend(footer_pill("/", "search"));
                        spans.extend(footer_pill("d", "deps"));
                        spans.extend(footer_pill("?", "help"));
                        spans.extend(footer_pill("q", "quit"));
                        spans.push(Span::styled(pos, Style::default().fg(COMMENT)));
                        Line::from(spans)
                    };
                    frame.render_widget(
                        Paragraph::new(footer_line).style(Style::default().bg(HEADER_BG)),
                        outer[3],
                    );

                    // ---- Help overlay ----
                    if matches!(mode, Mode::Help) {
                        let help_area = centered_rect(60, 70, size);
                        frame.render_widget(Clear, help_area);

                        let help_block = Block::default()
                            .borders(Borders::ALL)
                            .border_type(BorderType::Rounded)
                            .border_style(Style::default().fg(BLUE))
                            .title(Span::styled(
                                " Keyboard Shortcuts ",
                                Style::default()
                                    .fg(BLUE)
                                    .add_modifier(Modifier::BOLD),
                            ))
                            .style(Style::default().bg(Color::Rgb(0x1e, 0x22, 0x2a)));
                        let help_inner = help_block.inner(help_area);
                        frame.render_widget(help_block, help_area);

                        let mut help_lines: Vec<Line> = Vec::new();
                        for (key, desc) in HELP_LINES {
                            if key.is_empty() && desc.is_empty() {
                                help_lines.push(Line::from(""));
                            } else if desc.is_empty() {
                                help_lines.push(Line::from(Span::styled(
                                    format!(" {}", key),
                                    Style::default()
                                        .fg(BLUE)
                                        .add_modifier(Modifier::BOLD),
                                )));
                            } else {
                                let kw = 28;
                                let padded = format!("  {:width$}", key, width = kw);
                                help_lines.push(Line::from(vec![
                                    Span::styled(
                                        padded,
                                        Style::default()
                                            .fg(FG)
                                            .add_modifier(Modifier::BOLD),
                                    ),
                                    Span::styled(
                                        desc.to_string(),
                                        Style::default().fg(COMMENT),
                                    ),
                                ]));
                            }
                        }
                        help_lines.push(Line::from(""));
                        help_lines.push(Line::from(Span::styled(
                            "  Press ? or Esc to close",
                            Style::default().fg(COMMENT),
                        )));

                        frame.render_widget(Paragraph::new(help_lines), help_inner);
                    }
                }
            }
        })?;

        // =====================================================================
        // Input handling
        // =====================================================================

        let key = tui::read_key()?;

        match &mut mode {
            Mode::Help => match key.code {
                KeyCode::Char('?') | KeyCode::Esc | KeyCode::Char('q') => {
                    mode = Mode::Normal;
                }
                _ => {}
            },

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
                                state.detail_scroll = 0;
                                state.deps_cursor = 0;
                            }
                        }
                        KeyCode::Char('k') | KeyCode::Up => {
                            if state.cursor > 0 {
                                state.cursor -= 1;
                                state.detail_scroll = 0;
                                state.deps_cursor = 0;
                            }
                        }
                        KeyCode::Char('g') | KeyCode::Home => {
                            state.cursor = 0;
                            state.detail_scroll = 0;
                            state.deps_cursor = 0;
                        }
                        KeyCode::Char('G') | KeyCode::End => {
                            state.cursor = middle_count.saturating_sub(1);
                            state.detail_scroll = 0;
                            state.deps_cursor = 0;
                        }
                        KeyCode::PageDown => {
                            let page = terminal.size()?.height.saturating_sub(6) as usize;
                            state.cursor =
                                (state.cursor + page).min(middle_count.saturating_sub(1));
                            state.detail_scroll = 0;
                            state.deps_cursor = 0;
                        }
                        KeyCode::PageUp => {
                            let page = terminal.size()?.height.saturating_sub(6) as usize;
                            state.cursor = state.cursor.saturating_sub(page);
                            state.detail_scroll = 0;
                            state.deps_cursor = 0;
                        }
                        KeyCode::Char('l') | KeyCode::Right | KeyCode::Enter => {
                            if let Some(children) =
                                get_children_at_path(&root_children, &state.path)
                            {
                                if let Some((name, node)) = children.get(state.cursor) {
                                    if matches!(node, ConfigNode::Branch(_)) {
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
                                    }
                                }
                            }
                        }
                        KeyCode::Char('h') | KeyCode::Left => {
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
                            }
                        }
                        KeyCode::Char('d') => {
                            state.focus = Focus::Deps;
                            state.deps_cursor = 0;
                        }
                        KeyCode::Char('J') => {
                            state.detail_scroll += 1;
                        }
                        KeyCode::Char('K') => {
                            state.detail_scroll = state.detail_scroll.saturating_sub(1);
                        }
                        KeyCode::Char('/') => {
                            mode = Mode::Search {
                                query: String::new(),
                                results: Vec::new(),
                                cursor: 0,
                                scroll: 0,
                            };
                        }
                        KeyCode::Char('?') => {
                            mode = Mode::Help;
                        }
                        _ => {}
                    },

                    Focus::Deps => {
                        let full_path: Vec<String> = {
                            let mut p = state.path.clone();
                            if let Some(children) =
                                get_children_at_path(&root_children, &state.path)
                            {
                                if let Some((name, _)) = children.get(state.cursor) {
                                    p.push(name.clone());
                                }
                            }
                            p
                        };
                        let path_str = full_path.join(".");
                        let total = deps_total_count(&path_str, &deps_index);

                        match key.code {
                            KeyCode::Char('j') | KeyCode::Down => {
                                if total > 0 && state.deps_cursor + 1 < total {
                                    state.deps_cursor += 1;
                                }
                            }
                            KeyCode::Char('k') | KeyCode::Up => {
                                state.deps_cursor = state.deps_cursor.saturating_sub(1);
                            }
                            KeyCode::PageDown => {
                                let page =
                                    terminal.size()?.height.saturating_sub(8) as usize;
                                state.deps_cursor =
                                    (state.deps_cursor + page).min(total.saturating_sub(1));
                            }
                            KeyCode::PageUp => {
                                let page =
                                    terminal.size()?.height.saturating_sub(8) as usize;
                                state.deps_cursor =
                                    state.deps_cursor.saturating_sub(page);
                            }
                            KeyCode::Enter => {
                                if let Some(target) = dep_item_at(
                                    &path_str,
                                    &deps_index,
                                    state.deps_cursor,
                                ) {
                                    jump_to_path(&mut state, &target, &root_children);
                                    status_msg =
                                        Some(format!("Jumped to {}", target.join(".")));
                                }
                            }
                            KeyCode::Esc | KeyCode::Char('h') | KeyCode::Left => {
                                state.focus = Focus::Middle;
                            }
                            KeyCode::Char('q') => {
                                tui::teardown(terminal)?;
                                return Ok(());
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
            } => match key.code {
                KeyCode::Esc => {
                    mode = Mode::Normal;
                }
                KeyCode::Enter => {
                    if !results.is_empty() && *s_cursor < results.len() {
                        let target = results[*s_cursor].clone();
                        mode = Mode::Normal;
                        jump_to_path(&mut state, &target, &root_children);
                        status_msg = Some(format!("Jumped to {}", target.join(".")));
                    }
                }
                KeyCode::Down => {
                    if !results.is_empty() && *s_cursor + 1 < results.len() {
                        *s_cursor += 1;
                    }
                }
                KeyCode::Up => {
                    *s_cursor = s_cursor.saturating_sub(1);
                }
                KeyCode::PageDown => {
                    let page = terminal.size()?.height.saturating_sub(8) as usize;
                    *s_cursor =
                        (*s_cursor + page).min(results.len().saturating_sub(1));
                }
                KeyCode::PageUp => {
                    let page = terminal.size()?.height.saturating_sub(8) as usize;
                    *s_cursor = s_cursor.saturating_sub(page);
                }
                KeyCode::Backspace => {
                    query.pop();
                    *results = search_tree(&root_children, query, &[]);
                    *s_cursor = 0;
                    *s_scroll = 0;
                }
                KeyCode::Char(c) => {
                    query.push(c);
                    *results = search_tree(&root_children, query, &[]);
                    *s_cursor = 0;
                    *s_scroll = 0;
                }
                _ => {}
            },
        }
    }

    tui::teardown(terminal)?;
    Ok(())
}
