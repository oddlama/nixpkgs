use std::collections::HashMap;

use anyhow::Result;
use crossterm::event::{KeyCode, KeyModifiers, MouseEventKind, MouseButton};
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
    /// A node that appears in the dependency graph but has no serializable value.
    Phantom,
}

struct DepsIndex {
    dependencies: HashMap<String, Vec<String>>,
    dependents: HashMap<String, Vec<String>>,
}

#[derive(Clone, PartialEq)]
enum Focus {
    Middle,
    Detail,
    Deps,
    Revs,
}

struct MillerState {
    path: Vec<String>,
    cursor: usize,
    scroll: usize,
    path_memory: HashMap<Vec<String>, (usize, usize)>,
    detail_scroll: usize,
    deps_cursor: usize,
    deps_scroll: usize,
    focus: Focus,
}

enum Mode {
    Normal,
    Search {
        query: String,
        results: Vec<Vec<String>>,
        cursor: usize,
        scroll: usize,
        right_focus: Focus,
        detail_scroll: usize,
        deps_cursor: usize,
        deps_scroll: usize,
    },
    Help,
    Pager {
        path: Vec<String>,
        lines: Vec<String>,
        scroll: usize,
        color: Color,
    },
}

fn rect_contains(r: Rect, col: u16, row: u16) -> bool {
    col >= r.x && col < r.x + r.width && row >= r.y && row < r.y + r.height
}

/// Remembered inner areas for mouse hit-testing.
#[derive(Default)]
struct PaneAreas {
    // Normal mode
    parent_inner: Rect,
    browse_inner: Rect,
    children_inner: Rect,
    detail_inner: Rect,
    deps_inner: Rect,
    revs_inner: Rect,
    // Scroll offsets needed to convert row → item index
    browse_scroll: usize,
    deps_scroll: usize,
    // Normal mode: counts for bounds-checking
    browse_count: usize,
    deps_count: usize,
    revs_count: usize,
    // Search mode
    search_results_inner: Rect,
    search_results_scroll: usize,
    search_results_count: usize,
    search_detail_inner: Rect,
    search_deps_inner: Rect,
    search_revs_inner: Rect,
    search_deps_count: usize,
    search_revs_count: usize,
    search_deps_scroll: usize,
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
const ACTIVE_BORDER: Color = MAGENTA;
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
const ICON_PHANTOM: &str = "\u{f06a}";   // nf-fa-exclamation_circle
const ICON_PHANTOM_COLOR: Color = YELLOW;

fn node_icon(node: &ConfigNode) -> (&'static str, Color) {
    match node {
        ConfigNode::Branch(_) => (ICON_BRANCH, ICON_BRANCH_COLOR),
        ConfigNode::Phantom => (ICON_PHANTOM, ICON_PHANTOM_COLOR),
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
// Type-based colors
// ---------------------------------------------------------------------------

fn node_name_color(node: &ConfigNode) -> Color {
    match node {
        ConfigNode::Branch(_) => BLUE,
        ConfigNode::Phantom => YELLOW,
        ConfigNode::Leaf(val) => match val {
            Value::Bool(_) => CYAN,
            Value::String(_) => GREEN,
            Value::Number(_) => MAGENTA,
            Value::Null => COMMENT,
            Value::Array(_) => YELLOW,
            Value::Object(_) => COMMENT,
        },
    }
}

fn value_color(value: &Value) -> Color {
    match value {
        Value::Bool(true) => GREEN,
        Value::Bool(false) => RED,
        Value::Number(_) => MAGENTA,
        Value::String(_) => GREEN,
        Value::Null => COMMENT,
        Value::Array(_) => YELLOW,
        Value::Object(_) => COMMENT,
    }
}

// ---------------------------------------------------------------------------
// Tree building
// ---------------------------------------------------------------------------

fn sort_branches_first(children: &mut [(String, ConfigNode)]) {
    children.sort_by(|(a, na), (b, nb)| {
        let a_branch = matches!(na, ConfigNode::Branch(_));
        let b_branch = matches!(nb, ConfigNode::Branch(_));
        b_branch.cmp(&a_branch).then_with(|| a.cmp(b))
    });
}

fn build_config_tree(value: &Value) -> ConfigNode {
    match value {
        Value::Object(map) if map.is_empty() => ConfigNode::Leaf(value.clone()),
        Value::Object(map) => {
            let mut children: Vec<(String, ConfigNode)> = map
                .iter()
                .map(|(k, v)| (k.clone(), build_config_tree(v)))
                .collect();
            sort_branches_first(&mut children);
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
            ConfigNode::Leaf(_) | ConfigNode::Phantom => return None,
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

fn lookup_node<'a>(root: &'a [(String, ConfigNode)], path: &[String]) -> Option<&'a ConfigNode> {
    if path.is_empty() {
        return None;
    }
    get_node_at_path(root, &path[..path.len() - 1], &path[path.len() - 1])
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

/// Insert placeholder nodes for dependency paths not present in the config tree.
///
/// Some options (e.g. `assertions`) are not serializable, so they don't appear
/// in configValues. But they may participate in the dependency graph. We add
/// them as `Leaf(Null)` so they're navigable and their deps/revdeps are visible.
fn insert_phantom_nodes(
    root: &mut Vec<(String, ConfigNode)>,
    deps_index: &DepsIndex,
) {
    use std::collections::HashSet;

    // Collect every dot-path mentioned in the dependency graph
    let mut all_paths: HashSet<&str> = HashSet::new();
    for key in deps_index.dependencies.keys() {
        all_paths.insert(key.as_str());
    }
    for key in deps_index.dependents.keys() {
        all_paths.insert(key.as_str());
    }
    for vals in deps_index.dependencies.values() {
        for v in vals {
            all_paths.insert(v.as_str());
        }
    }
    for vals in deps_index.dependents.values() {
        for v in vals {
            all_paths.insert(v.as_str());
        }
    }

    for dot_path in all_paths {
        let segments: Vec<&str> = dot_path.split('.').collect();
        ensure_path_exists(root, &segments);
    }

    // Re-sort after insertions
    sort_tree_recursive(root);
}

/// Walk down `segments`, creating Branch/Leaf nodes as needed.
fn ensure_path_exists(root: &mut Vec<(String, ConfigNode)>, segments: &[&str]) {
    if segments.is_empty() {
        return;
    }

    let name = segments[0].to_string();
    let rest = &segments[1..];

    // Find or create the entry at this level
    let pos = root.iter().position(|(n, _)| *n == name);
    let idx = if let Some(i) = pos {
        i
    } else {
        // Insert new node
        if rest.is_empty() {
            root.push((name, ConfigNode::Phantom));
        } else {
            root.push((name, ConfigNode::Branch(Vec::new())));
        }
        root.len() - 1
    };

    if rest.is_empty() {
        return;
    }

    // Need to recurse into a branch — promote Leaf to Branch if necessary
    match &mut root[idx].1 {
        ConfigNode::Branch(children) => {
            ensure_path_exists(children, rest);
        }
        ConfigNode::Leaf(_) | ConfigNode::Phantom => {
            // This was a leaf/phantom but we need children below it — promote to branch
            let mut children = Vec::new();
            ensure_path_exists(&mut children, rest);
            root[idx].1 = ConfigNode::Branch(children);
        }
    }
}

fn sort_tree_recursive(children: &mut [(String, ConfigNode)]) {
    sort_branches_first(children);
    for (_, node) in children.iter_mut() {
        if let ConfigNode::Branch(ch) = node {
            sort_tree_recursive(ch);
        }
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
        Value::Array(arr) if arr.is_empty() => "[]".to_string(),
        Value::Array(arr) => format!("[{}]", arr.len()),
        Value::Object(map) if map.is_empty() => "{}".to_string(),
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
    state.deps_scroll = 0;

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

fn current_full_path(state: &MillerState, root: &[(String, ConfigNode)]) -> Vec<String> {
    let mut p = state.path.clone();
    if let Some(children) = get_children_at_path(root, &state.path) {
        if let Some((name, _)) = children.get(state.cursor) {
            p.push(name.clone());
        }
    }
    p
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

/// Block with a focus-key letter highlighted in bold red within the title.
fn make_block_keyed<'a>(base: &str, count: Option<usize>, key_char: char, active: bool) -> Block<'a> {
    let border_color = if active { ACTIVE_BORDER } else { INACTIVE_BORDER };
    let mut spans: Vec<Span<'a>> = vec![Span::raw(" ")];
    let mut found = false;
    for c in base.chars() {
        if !found && c.to_ascii_lowercase() == key_char {
            spans.push(Span::styled(
                c.to_string(),
                Style::default().fg(RED).add_modifier(Modifier::BOLD),
            ));
            found = true;
        } else {
            spans.push(Span::styled(
                c.to_string(),
                Style::default()
                    .fg(border_color)
                    .add_modifier(Modifier::BOLD),
            ));
        }
    }
    if let Some(n) = count {
        spans.push(Span::styled(
            format!(" ({})", n),
            Style::default()
                .fg(border_color)
                .add_modifier(Modifier::BOLD),
        ));
    }
    spans.push(Span::raw(" "));

    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_color))
        .title(Line::from(spans))
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
        let color = node_name_color(node);
        let (icon, icon_color) = node_icon(node);
        let is_branch = matches!(node, ConfigNode::Branch(_));

        let is_cursor = cursor_idx == Some(i);
        let is_highlight = highlight_name == Some(name.as_str());

        let bg = if is_cursor {
            CURSOR_BG
        } else if is_highlight {
            PARENT_HIGHLIGHT_BG
        } else {
            Color::Reset
        };

        let name_mod = if is_cursor || is_highlight || is_branch {
            Modifier::BOLD
        } else {
            Modifier::empty()
        };

        let name_style = Style::default().fg(color).bg(bg).add_modifier(name_mod);

        let suffix = match node {
            ConfigNode::Branch(ch) => {
                let count_str = format!("{{{}}}", ch.len());
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
            ConfigNode::Phantom => {
                ("!".to_string(), Style::default().fg(YELLOW).bg(bg))
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

/// Render a search result line with icon, parent segments in bold blue,
/// leaf segment in type color, and match highlighting overlay.
fn render_search_result_line<'a>(
    path: &[String],
    query: &str,
    is_selected: bool,
    _width: u16,
    node: Option<&ConfigNode>,
) -> Line<'a> {
    let (icon, icon_color) = node.map(node_icon).unwrap_or(("?", COMMENT));
    let leaf_color = node.map(node_name_color).unwrap_or(FG);
    let bg = if is_selected { CURSOR_BG } else { Color::Reset };

    let display = path.join(".");
    let lower_display = display.to_lowercase();
    let lower_query = query.to_lowercase();

    let highlight: Option<(usize, usize)> = if !lower_query.is_empty() {
        lower_display
            .find(&lower_query)
            .map(|s| (s, s + query.len()))
    } else {
        None
    };

    let mut spans: Vec<Span<'a>> = vec![
        Span::styled(format!("{} ", icon), Style::default().fg(icon_color).bg(bg)),
    ];

    // Render segment by segment: parents in bold blue, last in type color
    let mut pos: usize = 0;
    for (seg_idx, seg) in path.iter().enumerate() {
        if seg_idx > 0 {
            // Dot separator
            let dot_in_hl = highlight
                .map(|(hs, he)| pos >= hs && pos < he)
                .unwrap_or(false);
            let dot_style = if dot_in_hl {
                Style::default()
                    .fg(Color::White)
                    .bg(HIGHLIGHT_BG)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(COMMENT).bg(bg)
            };
            spans.push(Span::styled(".", dot_style));
            pos += 1;
        }

        let is_last = seg_idx == path.len() - 1;
        let seg_color = if is_last { leaf_color } else { BLUE };
        let seg_mod = Modifier::BOLD; // bold for both parents and leaf

        let seg_start = pos;
        let seg_end = pos + seg.len();

        if let Some((hs, he)) = highlight {
            if hs < seg_end && he > seg_start {
                // Overlap with highlight
                let hl_start = hs.max(seg_start) - seg_start;
                let hl_end = he.min(seg_end) - seg_start;

                if hl_start > 0 {
                    spans.push(Span::styled(
                        seg[..hl_start].to_string(),
                        Style::default()
                            .fg(seg_color)
                            .bg(bg)
                            .add_modifier(seg_mod),
                    ));
                }
                spans.push(Span::styled(
                    seg[hl_start..hl_end].to_string(),
                    Style::default()
                        .fg(Color::White)
                        .bg(HIGHLIGHT_BG)
                        .add_modifier(Modifier::BOLD),
                ));
                if hl_end < seg.len() {
                    spans.push(Span::styled(
                        seg[hl_end..].to_string(),
                        Style::default()
                            .fg(seg_color)
                            .bg(bg)
                            .add_modifier(seg_mod),
                    ));
                }
            } else {
                spans.push(Span::styled(
                    seg.clone(),
                    Style::default()
                        .fg(seg_color)
                        .bg(bg)
                        .add_modifier(seg_mod),
                ));
            }
        } else {
            spans.push(Span::styled(
                seg.clone(),
                Style::default()
                    .fg(seg_color)
                    .bg(bg)
                    .add_modifier(seg_mod),
            ));
        }

        pos = seg_end;
    }

    Line::from(spans)
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
    let (icon, icon_color) = node_icon(node);
    let path_color = node_name_color(node);

    let mut content: Vec<Line<'a>> = Vec::new();

    content.push(Line::from(vec![
        Span::styled(format!("{} ", icon), Style::default().fg(icon_color)),
        Span::styled(
            full_path.join("."),
            Style::default()
                .fg(path_color)
                .add_modifier(Modifier::BOLD),
        ),
    ]));
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
        ConfigNode::Phantom => {
            content.push(Line::from(Span::styled(
                "! (not serializable or value not used)",
                Style::default().fg(YELLOW),
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
    inner_width: u16,
    root_children: &[(String, ConfigNode)],
) -> Vec<Line<'a>> {
    let width = inner_width as usize;
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

    let icon_col = 2; // icon + space
    let text_width = width.saturating_sub(icon_col);

    let end = items.len().min(scroll + visible_height);
    let mut lines = Vec::new();
    for i in scroll..end {
        let dep = &items[i];
        let is_selected = cursor == Some(i);
        let bg = if is_selected { CURSOR_BG } else { Color::Reset };

        let path_parts: Vec<String> = dep.split('.').map(|s| s.to_string()).collect();
        let node = lookup_node(root_children, &path_parts);

        let (icon, icon_color, name_color) = match node {
            Some(n) => {
                let (ic, ic_col) = node_icon(n);
                (ic, ic_col, node_name_color(n))
            }
            None => (ICON_OBJECT, COMMENT, COMMENT),
        };

        let name_mod = if is_selected {
            Modifier::BOLD
        } else {
            Modifier::empty()
        };

        let suffix = match node {
            Some(ConfigNode::Branch(ch)) => {
                let count_str = format!("{{{}}}", ch.len());
                (count_str, Style::default().fg(COMMENT).bg(bg))
            }
            Some(ConfigNode::Leaf(val)) => {
                let short = format_value_short(val);
                let max_suffix = text_width / 2;
                let short = if max_suffix > 3 && short.len() > max_suffix {
                    format!("{}...", &short[..max_suffix - 3])
                } else {
                    short
                };
                (short, Style::default().fg(value_color(val)).bg(bg))
            }
            Some(ConfigNode::Phantom) => {
                ("!".to_string(), Style::default().fg(YELLOW).bg(bg))
            }
            None => (String::new(), Style::default().bg(bg)),
        };

        let name_display = if text_width > 4 && dep.len() + 1 + suffix.0.len() > text_width && dep.len() > 3 {
            let max_name = text_width.saturating_sub(suffix.0.len() + 4);
            if max_name > 3 {
                format!("{}...", &dep[..max_name])
            } else {
                dep.clone()
            }
        } else {
            dep.clone()
        };

        let name_len = name_display.len();
        let suffix_len = suffix.0.len();
        let padding = if suffix_len > 0 && name_len + suffix_len + 1 < text_width {
            text_width - name_len - suffix_len
        } else if suffix_len > 0 {
            1
        } else {
            0
        };

        let mut spans = vec![
            Span::styled(
                format!("{} ", icon),
                Style::default().fg(icon_color).bg(bg),
            ),
            Span::styled(
                name_display,
                Style::default()
                    .fg(name_color)
                    .bg(bg)
                    .add_modifier(name_mod),
            ),
        ];
        if suffix_len > 0 {
            spans.push(Span::styled(" ".repeat(padding), Style::default().bg(bg)));
            spans.push(Span::styled(suffix.0, suffix.1));
        }
        lines.push(Line::from(spans));
    }

    while lines.len() < visible_height {
        lines.push(Line::from(""));
    }
    lines
}


fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let v = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Min(0),
        ])
        .split(area);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Min(0),
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
            ConfigNode::Phantom => {
                println!("{}{} = !", indent, name);
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
    ("l / \u{2192} / Enter", "Drill into branch / view leaf"),
    ("h / \u{2190} / Esc", "Go back one level"),
    ("g / Home", "Jump to top"),
    ("G / End", "Jump to bottom"),
    ("PageDown / PageUp", "Page scroll"),
    ("", ""),
    ("Info Panes", ""),
    ("b", "Focus Browse pane"),
    ("d", "Focus Detail pane"),
    ("p", "Focus Dependencies pane"),
    ("n", "Focus Dependents pane"),
    ("J / K", "Quick-scroll detail"),
    ("j / k (in pane)", "Navigate / scroll"),
    ("Enter (in deps/revs)", "Jump to dependency"),
    ("Esc / h (in pane)", "Return to browse"),
    ("", ""),
    ("Value Pager", ""),
    ("Enter / \u{2192} on leaf", "Open fullscreen pager"),
    ("j / k / \u{2191}\u{2193}", "Scroll value"),
    ("Esc / h / \u{2190}", "Close pager"),
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

pub fn run(config: &str, explicit: bool, use_color: bool, nix_args: &[String]) -> Result<()> {
    if !use_color {
        let json = resolve::resolve(config, explicit, nix_args)?;
        let root_children: Vec<(String, ConfigNode)> = match &json {
            Value::Object(map) => {
                let mut children: Vec<(String, ConfigNode)> = map
                    .iter()
                    .map(|(k, v)| (k.clone(), build_config_tree(v)))
                    .collect();
                sort_branches_first(&mut children);
                children
            }
            _ => vec![("config".to_string(), build_config_tree(&json))],
        };
        print_tree_text(&root_children, 0);
        return Ok(());
    }

    let combined = resolve::resolve_combined(config, explicit, nix_args)?;
    let json = combined.config_values;

    let mut root_children: Vec<(String, ConfigNode)> = match &json {
        Value::Object(map) => {
            let mut children: Vec<(String, ConfigNode)> = map
                .iter()
                .map(|(k, v)| (k.clone(), build_config_tree(v)))
                .collect();
            sort_branches_first(&mut children);
            children
        }
        _ => vec![("config".to_string(), build_config_tree(&json))],
    };

    let deps_index = build_deps_index(&combined.filtered_deps);
    insert_phantom_nodes(&mut root_children, &deps_index);

    let mut state = MillerState {
        path: Vec::new(),
        cursor: 0,
        scroll: 0,
        path_memory: HashMap::new(),
        detail_scroll: 0,
        deps_cursor: 0,
        deps_scroll: 0,
        focus: Focus::Middle,
    };

    let mut mode = Mode::Normal;
    let mut status_msg: Option<String> = None;
    let mut pane_areas = PaneAreas::default();

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
                // Pager layout (fullscreen value viewer)
                // =============================================================
                Mode::Pager {
                    path,
                    lines: pager_lines,
                    scroll: p_scroll,
                    color: p_color,
                } => {
                    let outer = Layout::default()
                        .direction(Direction::Vertical)
                        .constraints([Constraint::Min(1), Constraint::Length(1)])
                        .split(size);

                    let title = path.join(".");
                    let block = Block::default()
                        .borders(Borders::ALL)
                        .border_type(BorderType::Rounded)
                        .border_style(Style::default().fg(BLUE))
                        .title(Span::styled(
                            format!(" {} ", title),
                            Style::default().fg(BLUE).add_modifier(Modifier::BOLD),
                        ));
                    let inner = block.inner(outer[0]);
                    let h = inner.height as usize;

                    let sc = (*p_scroll).min(pager_lines.len().saturating_sub(1));
                    let end = pager_lines.len().min(sc + h);
                    let mut content_lines: Vec<Line> = Vec::new();
                    for i in sc..end {
                        content_lines.push(Line::from(Span::styled(
                            pager_lines[i].clone(),
                            Style::default().fg(*p_color),
                        )));
                    }
                    while content_lines.len() < h {
                        content_lines.push(Line::from(""));
                    }

                    frame.render_widget(block, outer[0]);
                    frame.render_widget(Paragraph::new(content_lines), inner);

                    // Footer
                    let total = pager_lines.len();
                    let mut footer_spans = vec![Span::raw(" ")];
                    footer_spans.extend(footer_pill("j/k", "scroll"));
                    footer_spans.extend(footer_pill("g/G", "top/bottom"));
                    footer_spans.extend(footer_pill("Esc", "back"));
                    footer_spans.push(Span::styled(
                        format!("[{}/{}]", sc + 1, total.max(1)),
                        Style::default().fg(COMMENT),
                    ));
                    frame.render_widget(
                        Paragraph::new(Line::from(footer_spans))
                            .style(Style::default().bg(HEADER_BG)),
                        outer[1],
                    );
                }

                // =============================================================
                // Search layout
                // =============================================================
                Mode::Search {
                    query,
                    results,
                    cursor: s_cursor,
                    scroll: s_scroll,
                    right_focus,
                    detail_scroll: s_detail_scroll,
                    deps_cursor: s_deps_cursor,
                    deps_scroll: s_deps_scroll,
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

                    // Body: Results (60%) + Right panes (40%)
                    let body = Layout::default()
                        .direction(Direction::Horizontal)
                        .constraints([
                            Constraint::Percentage(60),
                            Constraint::Min(0),
                        ])
                        .split(outer[1]);

                    let results_active = *right_focus == Focus::Middle;
                    let results_block = make_block("Results", results_active);
                    let results_inner = results_block.inner(body[0]);
                    let results_height = results_inner.height as usize;
                    let results_width = results_inner.width;

                    let mut sc = *s_scroll;
                    let mut cu = *s_cursor;
                    clamp_cursor(&mut cu, &mut sc, total, results_height);
                    let end = total.min(sc + results_height);

                    let mut result_lines = Vec::new();
                    for i in sc..end {
                        let node = lookup_node(&root_children, &results[i]);
                        result_lines.push(render_search_result_line(
                            &results[i],
                            query,
                            i == cu,
                            results_width,
                            node,
                        ));
                    }
                    while result_lines.len() < results_height {
                        result_lines.push(Line::from(""));
                    }
                    frame.render_widget(results_block, body[0]);
                    frame.render_widget(Paragraph::new(result_lines), results_inner);

                    // Right side: Detail (40%) / Deps (30%) / Revs (30%)
                    let right_stack = Layout::default()
                        .direction(Direction::Vertical)
                        .constraints([
                            Constraint::Percentage(40),
                            Constraint::Percentage(30),
                            Constraint::Min(0),
                        ])
                        .split(body[1]);

                    // Resolve selected node info
                    let selected_info: Option<(&[String], &ConfigNode)> =
                        if !results.is_empty() && cu < results.len() {
                            let rp = &results[cu];
                            let parent = &rp[..rp.len() - 1];
                            let name = &rp[rp.len() - 1];
                            get_node_at_path(&root_children, parent, name)
                                .map(|node| (rp.as_slice(), node))
                        } else {
                            None
                        };

                    let path_str = selected_info
                        .map(|(rp, _)| rp.join("."))
                        .unwrap_or_default();
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
                    let rev_count = rev_items.len();

                    let s_detail_active = *right_focus == Focus::Detail;
                    let s_deps_active = *right_focus == Focus::Deps;
                    let s_revs_active = *right_focus == Focus::Revs;

                    // Detail pane
                    let detail_block = make_block_keyed("Detail", None, 'd', s_detail_active);
                    let detail_inner = detail_block.inner(right_stack[0]);
                    let detail_height = detail_inner.height as usize;
                    let detail_width = detail_inner.width;
                    let detail_lines = if let Some((rp, node)) = selected_info {
                        render_detail_info(
                            rp, node, *s_detail_scroll, detail_width, detail_height,
                        )
                    } else {
                        vec![Line::from(""); detail_height]
                    };
                    frame.render_widget(detail_block, right_stack[0]);
                    frame.render_widget(Paragraph::new(detail_lines), detail_inner);

                    // Dependencies pane
                    let deps_block = make_block_keyed("Dependencies", Some(dep_count), 'p', s_deps_active);
                    let deps_inner = deps_block.inner(right_stack[1]);
                    let deps_height = deps_inner.height as usize;
                    let (deps_cursor_val, deps_scroll_val) = if s_deps_active {
                        let mut dc = *s_deps_cursor;
                        let mut ds = *s_deps_scroll;
                        clamp_cursor(&mut dc, &mut ds, dep_count, deps_height);
                        (Some(dc), ds)
                    } else {
                        (None, 0)
                    };
                    let deps_lines =
                        render_dep_list(&dep_items, deps_cursor_val, deps_scroll_val, deps_height, deps_inner.width, &root_children);
                    frame.render_widget(deps_block, right_stack[1]);
                    frame.render_widget(Paragraph::new(deps_lines), deps_inner);

                    // Dependents pane
                    let rev_block = make_block_keyed("Dependents", Some(rev_count), 'n', s_revs_active);
                    let rev_inner = rev_block.inner(right_stack[2]);
                    let rev_height = rev_inner.height as usize;
                    let (revs_cursor_val, revs_scroll_val) = if s_revs_active {
                        let mut dc = *s_deps_cursor;
                        let mut ds = *s_deps_scroll;
                        clamp_cursor(&mut dc, &mut ds, rev_count, rev_height);
                        (Some(dc), ds)
                    } else {
                        (None, 0)
                    };
                    let rev_lines =
                        render_dep_list(&rev_items, revs_cursor_val, revs_scroll_val, rev_height, rev_inner.width, &root_children);
                    frame.render_widget(rev_block, right_stack[2]);
                    frame.render_widget(Paragraph::new(rev_lines), rev_inner);

                    pane_areas.search_results_inner = results_inner;
                    pane_areas.search_results_scroll = sc;
                    pane_areas.search_results_count = total;
                    pane_areas.search_detail_inner = detail_inner;
                    pane_areas.search_deps_inner = deps_inner;
                    pane_areas.search_revs_inner = rev_inner;
                    pane_areas.search_deps_count = dep_count;
                    pane_areas.search_revs_count = rev_count;
                    pane_areas.search_deps_scroll = *s_deps_scroll;

                    // Search bar
                    let search_active = *right_focus == Focus::Middle;
                    let search_block = make_block("Search", search_active);
                    let search_inner = search_block.inner(outer[2]);
                    let search_line = Line::from(vec![
                        Span::styled(query.as_str(), Style::default().fg(FG)),
                        if search_active {
                            Span::styled("\u{2588}", Style::default().fg(BLUE))
                        } else {
                            Span::raw("")
                        },
                    ]);
                    frame.render_widget(search_block, outer[2]);
                    frame.render_widget(Paragraph::new(search_line), search_inner);

                    // Footer
                    let mut footer_spans = vec![Span::raw(" ")];
                    if *right_focus == Focus::Middle {
                        footer_spans.extend(footer_pill("\u{2191}\u{2193}", "select"));
                        footer_spans.extend(footer_pill("Enter", "jump"));
                        footer_spans.extend(footer_pill("Tab", "panes"));
                        footer_spans.extend(footer_pill("Esc", "close"));
                    } else {
                        footer_spans.extend(footer_pill("d/p/n", "switch pane"));
                        footer_spans.extend(footer_pill("j/k", "scroll"));
                        if s_deps_active || s_revs_active {
                            footer_spans.extend(footer_pill("Enter", "jump"));
                        }
                        footer_spans.extend(footer_pill("Esc", "results"));
                    }
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
                            Constraint::Min(0),
                            Constraint::Length(1),
                        ])
                        .split(size);

                    // ---- Header / Breadcrumb ----
                    let selected: Option<(&str, &ConfigNode)> =
                        middle_children.and_then(|ch| {
                            ch.get(state.cursor)
                                .map(|(n, node)| (n.as_str(), node))
                        });

                    let mut header_spans: Vec<Span> = vec![Span::styled(
                        format!(" {} ", config),
                        Style::default().fg(FG).add_modifier(Modifier::BOLD),
                    )];
                    if !state.path.is_empty() {
                        header_spans.push(Span::styled(" ", Style::default()));
                        for (i, seg) in state.path.iter().enumerate() {
                            if i > 0 {
                                header_spans.push(Span::styled(
                                    ".",
                                    Style::default().fg(COMMENT),
                                ));
                            }
                            // Path segments are always branches
                            header_spans.push(Span::styled(
                                seg.clone(),
                                Style::default()
                                    .fg(BLUE)
                                    .add_modifier(Modifier::BOLD),
                            ));
                        }
                    }
                    // Append currently selected item to breadcrumb
                    if let Some((name, node)) = selected {
                        if state.path.is_empty() {
                            header_spans.push(Span::styled(" ", Style::default()));
                        } else {
                            header_spans.push(Span::styled(
                                ".",
                                Style::default().fg(COMMENT),
                            ));
                        }
                        let nc = node_name_color(node);
                        let nm = if matches!(node, ConfigNode::Branch(_)) {
                            Modifier::BOLD
                        } else {
                            Modifier::empty()
                        };
                        header_spans.push(Span::styled(
                            name.to_string(),
                            Style::default().fg(nc).add_modifier(nm),
                        ));
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
                            Constraint::Min(0),
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
                    pane_areas.parent_inner = left_inner;
                    frame.render_widget(left_block, top[0]);
                    frame.render_widget(Paragraph::new(left_lines), left_inner);

                    // Browse
                    let middle_active = state.focus == Focus::Middle;
                    let middle_block = make_block_keyed("Browse", None, 'b', middle_active);
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
                            None,
                            Some(state.cursor),
                            state.scroll,
                            middle_height,
                            middle_width,
                        )
                    } else {
                        vec![Line::from(""); middle_height]
                    };
                    pane_areas.browse_inner = middle_inner;
                    pane_areas.browse_scroll = state.scroll;
                    pane_areas.browse_count = middle_count;
                    frame.render_widget(middle_block, top[1]);
                    frame.render_widget(Paragraph::new(middle_lines), middle_inner);

                    // Children
                    let children_block = make_block("Children", false);
                    let children_inner = children_block.inner(top[2]);
                    let children_height = children_inner.height as usize;
                    let children_width = children_inner.width;

                    let children_lines =
                        if let Some((_name, ConfigNode::Branch(ch))) = selected {
                            render_pane_list(
                                ch,
                                None,
                                None,
                                0,
                                children_height,
                                children_width,
                            )
                        } else {
                            vec![Line::from(""); children_height]
                        };
                    pane_areas.children_inner = children_inner;
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
                    let rev_count = rev_items.len();

                    let detail_active = state.focus == Focus::Detail;
                    let deps_active = state.focus == Focus::Deps;
                    let revs_active = state.focus == Focus::Revs;

                    let narrow = screen_width < 110;

                    if narrow {
                        // Narrow: Detail (50%) | stacked Deps+Revs (50%)
                        let bottom = Layout::default()
                            .direction(Direction::Horizontal)
                            .constraints([
                                Constraint::Percentage(50),
                                Constraint::Min(0),
                            ])
                            .split(outer[2]);

                        // Detail
                        let detail_block = make_block_keyed("Detail", None, 'd', detail_active);
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
                                Constraint::Min(0),
                            ])
                            .split(bottom[1]);

                        // Dependencies
                        let deps_block = make_block_keyed("Dependencies", Some(dep_count), 'p', deps_active);
                        let deps_inner = deps_block.inner(right_stack[0]);
                        let deps_height = deps_inner.height as usize;
                        let deps_cursor_val = if deps_active {
                            clamp_cursor(&mut state.deps_cursor, &mut state.deps_scroll, dep_count, deps_height);
                            Some(state.deps_cursor)
                        } else {
                            None
                        };
                        let deps_lines =
                            render_dep_list(&dep_items, deps_cursor_val, state.deps_scroll, deps_height, deps_inner.width, &root_children);
                        frame.render_widget(deps_block, right_stack[0]);
                        frame.render_widget(Paragraph::new(deps_lines), deps_inner);

                        // Dependents
                        let rev_block = make_block_keyed("Dependents", Some(rev_count), 'n', revs_active);
                        let rev_inner = rev_block.inner(right_stack[1]);
                        let rev_height = rev_inner.height as usize;
                        let revs_cursor_val = if revs_active {
                            clamp_cursor(&mut state.deps_cursor, &mut state.deps_scroll, rev_count, rev_height);
                            Some(state.deps_cursor)
                        } else {
                            None
                        };
                        let rev_lines =
                            render_dep_list(&rev_items, revs_cursor_val, state.deps_scroll, rev_height, rev_inner.width, &root_children);
                        frame.render_widget(rev_block, right_stack[1]);
                        frame.render_widget(Paragraph::new(rev_lines), rev_inner);

                        pane_areas.detail_inner = detail_inner;
                        pane_areas.deps_inner = deps_inner;
                        pane_areas.revs_inner = rev_inner;
                        pane_areas.deps_scroll = state.deps_scroll;
                        pane_areas.deps_count = dep_count;
                        pane_areas.revs_count = rev_count;
                    } else {
                        // Wide: Detail (50%) | Dependencies (25%) | Dependents (25%)
                        let bottom = Layout::default()
                            .direction(Direction::Horizontal)
                            .constraints([
                                Constraint::Percentage(50),
                                Constraint::Percentage(25),
                                Constraint::Min(0),
                            ])
                            .split(outer[2]);

                        // Detail
                        let detail_block = make_block_keyed("Detail", None, 'd', detail_active);
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
                        let deps_block = make_block_keyed("Dependencies", Some(dep_count), 'p', deps_active);
                        let deps_inner = deps_block.inner(bottom[1]);
                        let deps_height = deps_inner.height as usize;
                        let deps_cursor_val = if deps_active {
                            clamp_cursor(&mut state.deps_cursor, &mut state.deps_scroll, dep_count, deps_height);
                            Some(state.deps_cursor)
                        } else {
                            None
                        };
                        let deps_lines =
                            render_dep_list(&dep_items, deps_cursor_val, state.deps_scroll, deps_height, deps_inner.width, &root_children);
                        frame.render_widget(deps_block, bottom[1]);
                        frame.render_widget(Paragraph::new(deps_lines), deps_inner);

                        // Dependents
                        let rev_block = make_block_keyed("Dependents", Some(rev_count), 'n', revs_active);
                        let rev_inner = rev_block.inner(bottom[2]);
                        let rev_height = rev_inner.height as usize;
                        let revs_cursor_val = if revs_active {
                            clamp_cursor(&mut state.deps_cursor, &mut state.deps_scroll, rev_count, rev_height);
                            Some(state.deps_cursor)
                        } else {
                            None
                        };
                        let rev_lines =
                            render_dep_list(&rev_items, revs_cursor_val, state.deps_scroll, rev_height, rev_inner.width, &root_children);
                        frame.render_widget(rev_block, bottom[2]);
                        frame.render_widget(Paragraph::new(rev_lines), rev_inner);

                        pane_areas.detail_inner = detail_inner;
                        pane_areas.deps_inner = deps_inner;
                        pane_areas.revs_inner = rev_inner;
                        pane_areas.deps_scroll = state.deps_scroll;
                        pane_areas.deps_count = dep_count;
                        pane_areas.revs_count = rev_count;
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

        let input = tui::read_input()?;

        // Handle mouse events
        if let tui::InputEvent::Mouse(mouse) = &input {
            let col = mouse.column;
            let row = mouse.row;

            match mouse.kind {
                MouseEventKind::Down(MouseButton::Left) => {
                    match &mut mode {
                        Mode::Normal | Mode::Help => {
                            if matches!(mode, Mode::Help) {
                                mode = Mode::Normal;
                                continue;
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
                                // Drill into the currently selected branch
                                if let Some(children) = get_children_at_path(&root_children, &state.path) {
                                    if let Some((name, ConfigNode::Branch(ch))) = children.get(state.cursor) {
                                        let line = (row - pane_areas.children_inner.y) as usize;
                                        if line < ch.len() {
                                            // First drill into the branch
                                            state.path_memory.insert(
                                                state.path.clone(),
                                                (state.cursor, state.scroll),
                                            );
                                            state.path.push(name.clone());
                                            // Then select the clicked child
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
                                        // Already selected — jump
                                        let full_path = current_full_path(&state, &root_children);
                                        let path_str = full_path.join(".");
                                        if let Some(items) = deps_index.dependencies.get(&path_str) {
                                            if idx < items.len() {
                                                let target: Vec<String> = items[idx].split('.').map(|s| s.to_string()).collect();
                                                let msg = format!("Jumped to {}", target.join("."));
                                                jump_to_path(&mut state, &target, &root_children);
                                                status_msg = Some(msg);
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
                                        // Already selected — jump
                                        let full_path = current_full_path(&state, &root_children);
                                        let path_str = full_path.join(".");
                                        if let Some(items) = deps_index.dependents.get(&path_str) {
                                            if idx < items.len() {
                                                let target: Vec<String> = items[idx].split('.').map(|s| s.to_string()).collect();
                                                let msg = format!("Jumped to {}", target.join("."));
                                                jump_to_path(&mut state, &target, &root_children);
                                                status_msg = Some(msg);
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
                            // Deps pane: click to focus + select, or jump if already selected
                            else if rect_contains(pane_areas.search_deps_inner, col, row) {
                                let line = (row - pane_areas.search_deps_inner.y) as usize;
                                let idx = pane_areas.search_deps_scroll + line;
                                if idx < pane_areas.search_deps_count {
                                    if *right_focus == Focus::Deps && *s_deps_cursor == idx {
                                        // Already selected — jump
                                        let path_str = if !results.is_empty() && *s_cursor < results.len() {
                                            results[*s_cursor].join(".")
                                        } else {
                                            String::new()
                                        };
                                        if let Some(items) = deps_index.dependencies.get(&path_str) {
                                            if idx < items.len() {
                                                let target: Vec<String> = items[idx].split('.').map(|s| s.to_string()).collect();
                                                let msg = format!("Jumped to {}", target.join("."));
                                                mode = Mode::Normal;
                                                jump_to_path(&mut state, &target, &root_children);
                                                status_msg = Some(msg);
                                            }
                                        }
                                    } else {
                                        *s_deps_cursor = idx;
                                    }
                                }
                                if let Mode::Search { right_focus, .. } = &mut mode {
                                    *right_focus = Focus::Deps;
                                }
                            }
                            // Revs pane: click to focus + select, or jump if already selected
                            else if rect_contains(pane_areas.search_revs_inner, col, row) {
                                let line = (row - pane_areas.search_revs_inner.y) as usize;
                                let idx = pane_areas.search_deps_scroll + line;
                                if idx < pane_areas.search_revs_count {
                                    if *right_focus == Focus::Revs && *s_deps_cursor == idx {
                                        // Already selected — jump
                                        let path_str = if !results.is_empty() && *s_cursor < results.len() {
                                            results[*s_cursor].join(".")
                                        } else {
                                            String::new()
                                        };
                                        if let Some(items) = deps_index.dependents.get(&path_str) {
                                            if idx < items.len() {
                                                let target: Vec<String> = items[idx].split('.').map(|s| s.to_string()).collect();
                                                let msg = format!("Jumped to {}", target.join("."));
                                                mode = Mode::Normal;
                                                jump_to_path(&mut state, &target, &root_children);
                                                status_msg = Some(msg);
                                            }
                                        }
                                    } else {
                                        *s_deps_cursor = idx;
                                    }
                                }
                                if let Mode::Search { right_focus, .. } = &mut mode {
                                    *right_focus = Focus::Revs;
                                }
                            }
                        }
                        Mode::Pager { .. } => {}
                    }
                    continue;
                }
                MouseEventKind::ScrollDown => {
                    match &mut mode {
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
                                let full_path = current_full_path(&state, &root_children);
                                let path_str = full_path.join(".");
                                let total = deps_index.dependencies.get(&path_str).map(|v| v.len()).unwrap_or(0);
                                if total > 0 && state.deps_cursor + 1 < total {
                                    state.deps_cursor += 1;
                                }
                            } else if rect_contains(pane_areas.revs_inner, col, row) {
                                let full_path = current_full_path(&state, &root_children);
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
                    continue;
                }
                MouseEventKind::ScrollUp => {
                    match &mut mode {
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
                    continue;
                }
                _ => { continue; }
            }
        }

        let key = match input {
            tui::InputEvent::Key(k) => k,
            _ => continue,
        };

        // Global Ctrl-C quit from any mode
        if key.code == KeyCode::Char('c')
            && key.modifiers.contains(KeyModifiers::CONTROL)
        {
            break;
        }

        match &mut mode {
            Mode::Help => match key.code {
                KeyCode::Char('?') | KeyCode::Esc | KeyCode::Char('q') => {
                    mode = Mode::Normal;
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
                    let page = terminal.size()?.height.saturating_sub(4) as usize;
                    *p_scroll =
                        (*p_scroll + page).min(pager_lines.len().saturating_sub(1));
                }
                KeyCode::PageUp => {
                    let page = terminal.size()?.height.saturating_sub(4) as usize;
                    *p_scroll = p_scroll.saturating_sub(page);
                }
                KeyCode::Esc
                | KeyCode::Char('q')
                | KeyCode::Char('h')
                | KeyCode::Left
                | KeyCode::Backspace => {
                    mode = Mode::Normal;
                }
                _ => {}
            },

            Mode::Normal => {
                status_msg = None;
                if key.code == KeyCode::Char('q') {
                    break;
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
                            let page = terminal.size()?.height.saturating_sub(6) as usize;
                            state.cursor =
                                (state.cursor + page).min(middle_count.saturating_sub(1));
                            state.detail_scroll = 0;
                            state.deps_cursor = 0;
                            state.deps_scroll = 0;
                        }
                        KeyCode::PageUp => {
                            let page = terminal.size()?.height.saturating_sub(6) as usize;
                            state.cursor = state.cursor.saturating_sub(page);
                            state.detail_scroll = 0;
                            state.deps_cursor = 0;
                            state.deps_scroll = 0;
                        }
                        KeyCode::Char('l') | KeyCode::Right | KeyCode::Enter => {
                            if let Some(children) =
                                get_children_at_path(&root_children, &state.path)
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
                                            mode = Mode::Pager {
                                                path: full,
                                                lines,
                                                scroll: 0,
                                                color,
                                            };
                                        }
                                        ConfigNode::Phantom => {
                                            let mut full = state.path.clone();
                                            full.push(name.clone());
                                            mode = Mode::Pager {
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
                            mode = Mode::Search {
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
                            mode = Mode::Help;
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
                                terminal.size()?.height.saturating_sub(8) as usize;
                            state.detail_scroll += page;
                        }
                        KeyCode::PageUp => {
                            let page =
                                terminal.size()?.height.saturating_sub(8) as usize;
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
                        let full_path = current_full_path(&state, &root_children);
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
                                    terminal.size()?.height.saturating_sub(8) as usize;
                                state.deps_cursor = (state.deps_cursor + page)
                                    .min(total.saturating_sub(1));
                            }
                            KeyCode::PageUp => {
                                let page =
                                    terminal.size()?.height.saturating_sub(8) as usize;
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
                                        &mut state,
                                        &target,
                                        &root_children,
                                    );
                                    status_msg = Some(msg);
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
                        let full_path = current_full_path(&state, &root_children);
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
                                    terminal.size()?.height.saturating_sub(8) as usize;
                                state.deps_cursor = (state.deps_cursor + page)
                                    .min(total.saturating_sub(1));
                            }
                            KeyCode::PageUp => {
                                let page =
                                    terminal.size()?.height.saturating_sub(8) as usize;
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
                                        &mut state,
                                        &target,
                                        &root_children,
                                    );
                                    status_msg = Some(msg);
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
                        let page = terminal.size()?.height.saturating_sub(8) as usize;
                        *s_cursor =
                            (*s_cursor + page).min(results.len().saturating_sub(1));
                        *s_detail_scroll = 0;
                        *s_deps_cursor = 0;
                        *s_deps_scroll = 0;
                    }
                    KeyCode::PageUp => {
                        let page = terminal.size()?.height.saturating_sub(8) as usize;
                        *s_cursor = s_cursor.saturating_sub(page);
                        *s_detail_scroll = 0;
                        *s_deps_cursor = 0;
                        *s_deps_scroll = 0;
                    }
                    KeyCode::Backspace => {
                        query.pop();
                        *results = search_tree(&root_children, query, &[]);
                        *s_cursor = 0;
                        *s_scroll = 0;
                        *s_detail_scroll = 0;
                        *s_deps_cursor = 0;
                        *s_deps_scroll = 0;
                    }
                    KeyCode::Char(c) => {
                        query.push(c);
                        *results = search_tree(&root_children, query, &[]);
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
                        let page = terminal.size()?.height.saturating_sub(8) as usize;
                        *s_detail_scroll += page;
                    }
                    KeyCode::PageUp => {
                        let page = terminal.size()?.height.saturating_sub(8) as usize;
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
                            let page = terminal.size()?.height.saturating_sub(8) as usize;
                            *s_deps_cursor = (*s_deps_cursor + page).min(dep_total.saturating_sub(1));
                        }
                        KeyCode::PageUp => {
                            let page = terminal.size()?.height.saturating_sub(8) as usize;
                            *s_deps_cursor = s_deps_cursor.saturating_sub(page);
                        }
                        KeyCode::Enter => {
                            if *s_deps_cursor < dep_total {
                                let target: Vec<String> = dep_items[*s_deps_cursor]
                                    .split('.')
                                    .map(|s| s.to_string())
                                    .collect();
                                let msg = format!("Jumped to {}", target.join("."));
                                mode = Mode::Normal;
                                jump_to_path(&mut state, &target, &root_children);
                                status_msg = Some(msg);
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
                            let page = terminal.size()?.height.saturating_sub(8) as usize;
                            *s_deps_cursor = (*s_deps_cursor + page).min(rev_total.saturating_sub(1));
                        }
                        KeyCode::PageUp => {
                            let page = terminal.size()?.height.saturating_sub(8) as usize;
                            *s_deps_cursor = s_deps_cursor.saturating_sub(page);
                        }
                        KeyCode::Enter => {
                            if *s_deps_cursor < rev_total {
                                let target: Vec<String> = rev_items[*s_deps_cursor]
                                    .split('.')
                                    .map(|s| s.to_string())
                                    .collect();
                                let msg = format!("Jumped to {}", target.join("."));
                                mode = Mode::Normal;
                                jump_to_path(&mut state, &target, &root_children);
                                status_msg = Some(msg);
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
    }

    tui::teardown(terminal)?;
    Ok(())
}
