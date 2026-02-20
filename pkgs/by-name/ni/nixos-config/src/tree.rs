use anyhow::Result;
use crossterm::event::KeyCode;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use serde_json::Value;

use crate::resolve;
use crate::tui;

/// A node in the configuration tree.
struct TreeNode {
    name: String,
    content: NodeContent,
    expanded: bool,
}

enum NodeContent {
    Branch(Vec<TreeNode>),
    Leaf(String),
}

/// A flattened line for rendering, produced by walking expanded nodes.
struct FlatLine {
    depth: usize,
    /// Index path from root to this node, for mutation lookups.
    index_path: Vec<usize>,
    name: String,
    kind: FlatLineKind,
}

enum FlatLineKind {
    Branch { expanded: bool, child_count: usize },
    Leaf(String),
}

/// Build a tree from a JSON value.
fn build_tree(name: &str, value: &Value) -> TreeNode {
    match value {
        Value::Object(map) if map.is_empty() => TreeNode {
            name: name.to_string(),
            content: NodeContent::Leaf("{ }".to_string()),
            expanded: false,
        },
        Value::Object(map) => {
            let children: Vec<TreeNode> = map
                .iter()
                .map(|(k, v)| build_tree(k, v))
                .collect();
            TreeNode {
                name: name.to_string(),
                content: NodeContent::Branch(children),
                expanded: false,
            }
        }
        other => {
            let mut buf = String::new();
            // Format the leaf value as nix
            format_leaf_value(&mut buf, other);
            TreeNode {
                name: name.to_string(),
                content: NodeContent::Leaf(buf),
                expanded: false,
            }
        }
    }
}

/// Format a leaf value inline (simplified nix formatting).
fn format_leaf_value(buf: &mut String, value: &Value) {
    match value {
        Value::Null => buf.push_str("null"),
        Value::Bool(b) => buf.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => buf.push_str(&n.to_string()),
        Value::String(s) => {
            if s.contains('\n') {
                buf.push_str("''...''");
            } else {
                buf.push('"');
                buf.push_str(&s.replace('\\', "\\\\").replace('"', "\\\""));
                buf.push('"');
            }
        }
        Value::Array(arr) => {
            buf.push_str(&format!("[ ... ] ({} items)", arr.len()));
        }
        Value::Object(_) => buf.push_str("{ ... }"),
    }
}

/// Flatten the tree into renderable lines, only recursing into expanded branches.
fn flatten_tree(nodes: &[TreeNode], depth: usize, parent_path: &[usize]) -> Vec<FlatLine> {
    let mut lines = Vec::new();
    for (i, node) in nodes.iter().enumerate() {
        let mut index_path = parent_path.to_vec();
        index_path.push(i);

        match &node.content {
            NodeContent::Branch(children) => {
                lines.push(FlatLine {
                    depth,
                    index_path: index_path.clone(),
                    name: node.name.clone(),
                    kind: FlatLineKind::Branch {
                        expanded: node.expanded,
                        child_count: children.len(),
                    },
                });
                if node.expanded {
                    lines.extend(flatten_tree(children, depth + 1, &index_path));
                }
            }
            NodeContent::Leaf(val) => {
                lines.push(FlatLine {
                    depth,
                    index_path,
                    name: node.name.clone(),
                    kind: FlatLineKind::Leaf(val.clone()),
                });
            }
        }
    }
    lines
}

/// Get a mutable reference to a node by its index path.
fn get_node_mut<'a>(roots: &'a mut Vec<TreeNode>, index_path: &[usize]) -> Option<&'a mut TreeNode> {
    if index_path.is_empty() {
        return None;
    }
    let mut current_children = roots.as_mut_slice();
    for (i, &idx) in index_path.iter().enumerate() {
        if idx >= current_children.len() {
            return None;
        }
        if i == index_path.len() - 1 {
            return Some(&mut current_children[idx]);
        }
        match &mut current_children[idx].content {
            NodeContent::Branch(children) => {
                current_children = children.as_mut_slice();
            }
            NodeContent::Leaf(_) => return None,
        }
    }
    None
}

/// Recursively expand nodes whose name contains the query, including all ancestors.
/// Returns true if this node or any descendant matched.
fn expand_matching(nodes: &mut [TreeNode], query: &str) -> bool {
    let mut any_match = false;
    for node in nodes.iter_mut() {
        let name_matches = node.name.to_lowercase().contains(&query.to_lowercase());
        let child_matches = match &mut node.content {
            NodeContent::Branch(children) => expand_matching(children, query),
            NodeContent::Leaf(val) => val.to_lowercase().contains(&query.to_lowercase()),
        };
        if name_matches || child_matches {
            if matches!(node.content, NodeContent::Branch(_)) {
                node.expanded = true;
            }
            any_match = true;
        }
    }
    any_match
}

/// Collapse all nodes recursively.
fn collapse_all(nodes: &mut [TreeNode]) {
    for node in nodes.iter_mut() {
        node.expanded = false;
        if let NodeContent::Branch(children) = &mut node.content {
            collapse_all(children);
        }
    }
}

/// Print an indented text tree to stdout (non-TTY mode).
fn print_tree(nodes: &[TreeNode], depth: usize) {
    for node in nodes {
        let indent = "  ".repeat(depth);
        match &node.content {
            NodeContent::Branch(children) => {
                println!("{}{}/", indent, node.name);
                print_tree(children, depth + 1);
            }
            NodeContent::Leaf(val) => {
                println!("{}{} = {}", indent, node.name, val);
            }
        }
    }
}

pub fn run(config: &str, explicit: bool) -> Result<()> {
    let json = resolve::resolve(config, explicit)?;

    // Build tree from top-level object
    let mut roots: Vec<TreeNode> = match &json {
        Value::Object(map) => map.iter().map(|(k, v)| build_tree(k, v)).collect(),
        _ => vec![build_tree("config", &json)],
    };

    if !tui::is_tty() {
        print_tree(&roots, 0);
        return Ok(());
    }

    let mut terminal = tui::setup()?;
    let mut cursor: usize = 0;
    let mut scroll: usize = 0;
    let mut mode = Mode::Normal;
    let mut search_query = String::new();
    let mut status_msg: Option<String> = None;

    loop {
        let flat = flatten_tree(&roots, 0, &[]);
        let total = flat.len();

        // Clamp cursor
        if total == 0 {
            cursor = 0;
        } else if cursor >= total {
            cursor = total - 1;
        }

        terminal.draw(|frame| {
            let size = frame.area();
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(1), // header
                    Constraint::Min(1),    // content
                    Constraint::Length(1), // footer
                ])
                .split(size);

            // Header
            let header = Line::from(vec![
                Span::styled(
                    format!(" {} ", config),
                    Style::default().add_modifier(Modifier::BOLD),
                ),
                Span::raw(format!("  ({} visible)", total)),
            ]);
            frame.render_widget(
                Paragraph::new(header).style(Style::default().bg(Color::DarkGray)),
                chunks[0],
            );

            // Content
            let visible = chunks[1].height as usize;

            // Auto-scroll to keep cursor visible
            if cursor < scroll {
                scroll = cursor;
            } else if cursor >= scroll + visible {
                scroll = cursor - visible + 1;
            }

            let end = total.min(scroll + visible);

            let mut lines = Vec::new();
            for i in scroll..end {
                let fl = &flat[i];
                let indent = "  ".repeat(fl.depth);
                let is_cursor = i == cursor;

                let (icon, text) = match &fl.kind {
                    FlatLineKind::Branch {
                        expanded,
                        child_count,
                    } => {
                        let icon = if *expanded { "▼ " } else { "▶ " };
                        let text = format!("{} ({})", fl.name, child_count);
                        (icon, text)
                    }
                    FlatLineKind::Leaf(val) => ("  ", format!("{} = {}", fl.name, val)),
                };

                let line_style = if is_cursor {
                    Style::default()
                        .bg(Color::Rgb(40, 40, 80))
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };

                let icon_style = if matches!(fl.kind, FlatLineKind::Branch { .. }) {
                    line_style.fg(Color::Yellow)
                } else {
                    line_style
                };

                lines.push(Line::from(vec![
                    Span::styled(indent, line_style),
                    Span::styled(icon.to_string(), icon_style),
                    Span::styled(text, line_style),
                ]));
            }
            frame.render_widget(Paragraph::new(lines), chunks[1]);

            // Footer
            let footer_line = match &mode {
                Mode::Normal => {
                    if let Some(msg) = &status_msg {
                        Line::from(Span::styled(
                            format!(" {} ", msg),
                            Style::default().fg(Color::Green),
                        ))
                    } else {
                        Line::from(vec![
                            Span::styled(
                                " j/k",
                                Style::default().add_modifier(Modifier::BOLD),
                            ),
                            Span::raw(":move "),
                            Span::styled("Enter", Style::default().add_modifier(Modifier::BOLD)),
                            Span::raw(":toggle "),
                            Span::styled("h/l", Style::default().add_modifier(Modifier::BOLD)),
                            Span::raw(":collapse/expand "),
                            Span::styled("/", Style::default().add_modifier(Modifier::BOLD)),
                            Span::raw(":search "),
                            Span::styled("q", Style::default().add_modifier(Modifier::BOLD)),
                            Span::raw(":quit"),
                            Span::raw(format!("  [{}/{}]", cursor + 1, total)),
                        ])
                    }
                }
                Mode::Search => Line::from(vec![
                    Span::styled(" /", Style::default().add_modifier(Modifier::BOLD)),
                    Span::raw(&search_query),
                ]),
            };
            frame.render_widget(
                Paragraph::new(footer_line).style(Style::default().bg(Color::DarkGray)),
                chunks[2],
            );
        })?;

        let key = tui::read_key()?;

        match &mode {
            Mode::Normal => {
                status_msg = None;
                if tui::is_quit(&key) {
                    break;
                }
                match key.code {
                    KeyCode::Char('j') | KeyCode::Down => {
                        if total > 0 && cursor + 1 < total {
                            cursor += 1;
                        }
                    }
                    KeyCode::Char('k') | KeyCode::Up => {
                        cursor = cursor.saturating_sub(1);
                    }
                    KeyCode::PageDown => {
                        let visible =
                            terminal.size()?.height.saturating_sub(2) as usize;
                        cursor = (cursor + visible).min(total.saturating_sub(1));
                    }
                    KeyCode::PageUp => {
                        let visible =
                            terminal.size()?.height.saturating_sub(2) as usize;
                        cursor = cursor.saturating_sub(visible);
                    }
                    KeyCode::Char('g') | KeyCode::Home => {
                        cursor = 0;
                    }
                    KeyCode::Char('G') | KeyCode::End => {
                        cursor = total.saturating_sub(1);
                    }
                    KeyCode::Enter | KeyCode::Char('l') | KeyCode::Right => {
                        // Toggle expand on branch, no-op on leaf
                        if total > 0 {
                            let path = flat[cursor].index_path.clone();
                            if let Some(node) = get_node_mut(&mut roots, &path) {
                                if matches!(node.content, NodeContent::Branch(_)) {
                                    node.expanded = !node.expanded;
                                }
                            }
                        }
                    }
                    KeyCode::Char('h') | KeyCode::Left => {
                        // Collapse current branch, or move to parent
                        if total > 0 {
                            let path = flat[cursor].index_path.clone();
                            if let Some(node) = get_node_mut(&mut roots, &path) {
                                if matches!(node.content, NodeContent::Branch(_))
                                    && node.expanded
                                {
                                    node.expanded = false;
                                } else if path.len() > 1 {
                                    // Move to parent
                                    let parent_path = &path[..path.len() - 1];
                                    if let Some(parent_idx) = flat.iter().position(|f| {
                                        f.index_path == parent_path
                                    }) {
                                        cursor = parent_idx;
                                    }
                                }
                            } else if path.len() > 1 {
                                // Leaf: move to parent
                                let parent_path = &path[..path.len() - 1];
                                if let Some(parent_idx) =
                                    flat.iter().position(|f| f.index_path == parent_path)
                                {
                                    cursor = parent_idx;
                                }
                            }
                        }
                    }
                    KeyCode::Char('/') => {
                        mode = Mode::Search;
                        search_query.clear();
                    }
                    _ => {}
                }
            }
            Mode::Search => match key.code {
                KeyCode::Enter => {
                    mode = Mode::Normal;
                    if !search_query.is_empty() {
                        collapse_all(&mut roots);
                        let found = expand_matching(&mut roots, &search_query);
                        if found {
                            // Re-flatten and jump to first match
                            let new_flat = flatten_tree(&roots, 0, &[]);
                            if let Some(pos) = new_flat.iter().position(|f| {
                                f.name.to_lowercase().contains(&search_query.to_lowercase())
                                    || matches!(&f.kind, FlatLineKind::Leaf(v) if v.to_lowercase().contains(&search_query.to_lowercase()))
                            }) {
                                cursor = pos;
                            } else {
                                cursor = 0;
                            }
                            status_msg = Some(format!("Expanded matches for '{}'", search_query));
                        } else {
                            status_msg = Some(format!("No matches for '{}'", search_query));
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
        }
    }

    tui::teardown(terminal)?;
    Ok(())
}

enum Mode {
    Normal,
    Search,
}
