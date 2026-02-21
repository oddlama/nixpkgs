use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

use crate::theme::*;
use super::types::{ConfigNode, DiffContext, DiffTag};
use super::icons::*;
use super::data::*;

// ---------------------------------------------------------------------------
// Pane list rendering
// ---------------------------------------------------------------------------

pub(super) fn render_pane_list<'a>(
    children: &[(String, ConfigNode)],
    highlight_name: Option<&str>,
    cursor_idx: Option<usize>,
    scroll: usize,
    visible_height: usize,
    inner_width: u16,
    diff_ctx: Option<&DiffContext>,
    path_prefix: &[String],
) -> Vec<Line<'a>> {
    let width = inner_width as usize;
    if width < 4 {
        return vec![Line::from(""); visible_height];
    }
    let end = children.len().min(scroll + visible_height);
    let mut lines = Vec::new();

    let icon_col = 2;
    let text_width = width.saturating_sub(icon_col);

    for i in scroll..end {
        let (name, node) = &children[i];
        let color = node_name_color(node);
        let (icon, icon_color) = node_icon(node);
        let is_branch = matches!(node, ConfigNode::Branch(_));

        let is_cursor = cursor_idx == Some(i);
        let is_highlight = highlight_name == Some(name.as_str());

        // Determine diff tag
        let diff_tag = diff_ctx.map(|ctx| get_diff_tag(ctx, path_prefix, name, node));

        let bg = if is_cursor {
            match diff_tag {
                Some(DiffTag::Removed) => DIFF_DELETE_CURSOR_BG,
                Some(DiffTag::Added) => DIFF_INSERT_CURSOR_BG,
                Some(DiffTag::Modified) => DIFF_MODIFIED_CURSOR_BG,
                _ => CURSOR_BG,
            }
        } else if is_highlight {
            PARENT_HIGHLIGHT_BG
        } else {
            match diff_tag {
                Some(DiffTag::Removed) => DIFF_DELETE_BG,
                Some(DiffTag::Added) => DIFF_INSERT_BG,
                Some(DiffTag::Modified) => DIFF_MODIFIED_BG,
                _ => Color::Reset,
            }
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
pub(super) fn render_search_result_line<'a>(
    path: &[String],
    query: &str,
    is_selected: bool,
    _width: u16,
    node: Option<&ConfigNode>,
    diff_ctx: Option<&DiffContext>,
) -> Line<'a> {
    let (icon, icon_color) = node.map(node_icon).unwrap_or(("?", COMMENT));
    let leaf_color = node.map(node_name_color).unwrap_or(FG);

    // Determine diff tag for search results
    let diff_tag = if let Some(ctx) = diff_ctx {
        let dot_path = path.join(".");
        ctx.tags.get(&dot_path).copied()
    } else {
        None
    };

    let bg = if is_selected {
        match diff_tag {
            Some(DiffTag::Removed) => DIFF_DELETE_CURSOR_BG,
            Some(DiffTag::Added) => DIFF_INSERT_CURSOR_BG,
            Some(DiffTag::Modified) => DIFF_MODIFIED_CURSOR_BG,
            _ => CURSOR_BG,
        }
    } else {
        match diff_tag {
            Some(DiffTag::Removed) => DIFF_DELETE_BG,
            Some(DiffTag::Added) => DIFF_INSERT_BG,
            Some(DiffTag::Modified) => DIFF_MODIFIED_BG,
            _ => Color::Reset,
        }
    };

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

    let mut pos: usize = 0;
    for (seg_idx, seg) in path.iter().enumerate() {
        if seg_idx > 0 {
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
        let seg_mod = Modifier::BOLD;

        let seg_start = pos;
        let seg_end = pos + seg.len();

        if let Some((hs, he)) = highlight {
            if hs < seg_end && he > seg_start {
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

pub(super) fn render_detail_info<'a>(
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

/// Render diff-aware detail info showing old and new values.
pub(super) fn render_diff_detail_info<'a>(
    full_path: &[String],
    node: &ConfigNode,
    diff_ctx: &DiffContext,
    scroll: usize,
    inner_width: u16,
    visible_height: usize,
) -> Vec<Line<'a>> {
    let width = inner_width as usize;
    let (icon, icon_color) = node_icon(node);
    let path_color = node_name_color(node);
    let dot_path = full_path.join(".");
    let tag = diff_ctx.tags.get(&dot_path).copied().unwrap_or(DiffTag::Unchanged);

    let mut content: Vec<Line<'a>> = Vec::new();

    // Path header
    content.push(Line::from(vec![
        Span::styled(format!("{} ", icon), Style::default().fg(icon_color)),
        Span::styled(
            dot_path.clone(),
            Style::default()
                .fg(path_color)
                .add_modifier(Modifier::BOLD),
        ),
    ]));

    // Diff tag label
    let (tag_label, tag_color) = match tag {
        DiffTag::Added => ("[ADDED]", GREEN),
        DiffTag::Removed => ("[REMOVED]", RED),
        DiffTag::Modified => ("[MODIFIED]", BLUE),
        DiffTag::Unchanged => ("[UNCHANGED]", COMMENT),
    };
    content.push(Line::from(Span::styled(
        tag_label,
        Style::default().fg(tag_color).add_modifier(Modifier::BOLD),
    )));
    content.push(Line::from(""));

    let divider = "\u{2500}".repeat(width.min(30));

    match tag {
        DiffTag::Modified => {
            // Old Value
            content.push(Line::from(Span::styled(
                "Old Value",
                Style::default().fg(RED).add_modifier(Modifier::BOLD),
            )));
            content.push(Line::from(Span::styled(divider.clone(), Style::default().fg(COMMENT))));
            if let Some(old_val) = diff_ctx.old_values.get(&dot_path) {
                for line in format_value_full(old_val) {
                    content.push(Line::from(Span::styled(line, Style::default().fg(RED))));
                }
            } else {
                match node {
                    ConfigNode::Branch(children) => {
                        content.push(Line::from(Span::styled(
                            format!("{} children", children.len()),
                            Style::default().fg(COMMENT),
                        )));
                    }
                    _ => {
                        content.push(Line::from(Span::styled("(no old value)", Style::default().fg(COMMENT))));
                    }
                }
            }
            content.push(Line::from(""));

            // New Value
            content.push(Line::from(Span::styled(
                "New Value",
                Style::default().fg(GREEN).add_modifier(Modifier::BOLD),
            )));
            content.push(Line::from(Span::styled(divider, Style::default().fg(COMMENT))));
            if let Some(new_val) = diff_ctx.new_values.get(&dot_path) {
                for line in format_value_full(new_val) {
                    content.push(Line::from(Span::styled(line, Style::default().fg(GREEN))));
                }
            } else {
                match node {
                    ConfigNode::Branch(children) => {
                        content.push(Line::from(Span::styled(
                            format!("{} children", children.len()),
                            Style::default().fg(COMMENT),
                        )));
                    }
                    _ => {
                        content.push(Line::from(Span::styled("(no new value)", Style::default().fg(COMMENT))));
                    }
                }
            }
        }
        DiffTag::Added => {
            content.push(Line::from(Span::styled(
                "New Value",
                Style::default().fg(GREEN).add_modifier(Modifier::BOLD),
            )));
            content.push(Line::from(Span::styled(divider, Style::default().fg(COMMENT))));
            match node {
                ConfigNode::Leaf(val) => {
                    for line in format_value_full(val) {
                        content.push(Line::from(Span::styled(line, Style::default().fg(GREEN))));
                    }
                }
                ConfigNode::Branch(children) => {
                    content.push(Line::from(Span::styled(
                        format!("{} children", children.len()),
                        Style::default().fg(COMMENT),
                    )));
                }
                ConfigNode::Phantom => {
                    content.push(Line::from(Span::styled("!", Style::default().fg(YELLOW))));
                }
            }
        }
        DiffTag::Removed => {
            content.push(Line::from(Span::styled(
                "Old Value",
                Style::default().fg(RED).add_modifier(Modifier::BOLD),
            )));
            content.push(Line::from(Span::styled(divider, Style::default().fg(COMMENT))));
            if let Some(old_val) = diff_ctx.old_values.get(&dot_path) {
                for line in format_value_full(old_val) {
                    content.push(Line::from(Span::styled(line, Style::default().fg(RED))));
                }
            } else {
                match node {
                    ConfigNode::Leaf(val) => {
                        for line in format_value_full(val) {
                            content.push(Line::from(Span::styled(line, Style::default().fg(RED))));
                        }
                    }
                    ConfigNode::Branch(children) => {
                        content.push(Line::from(Span::styled(
                            format!("{} children", children.len()),
                            Style::default().fg(COMMENT),
                        )));
                    }
                    ConfigNode::Phantom => {
                        content.push(Line::from(Span::styled("!", Style::default().fg(YELLOW))));
                    }
                }
            }
        }
        DiffTag::Unchanged => {
            // Same as normal detail
            content.push(Line::from(Span::styled(
                "Value",
                Style::default().fg(FG).add_modifier(Modifier::BOLD),
            )));
            content.push(Line::from(Span::styled(divider, Style::default().fg(COMMENT))));
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

pub(super) fn render_dep_list<'a>(
    items: &[String],
    cursor: Option<usize>,
    scroll: usize,
    visible_height: usize,
    inner_width: u16,
    root_children: &[(String, ConfigNode)],
    diff_dep_tags: Option<&[DiffTag]>,
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

    let icon_col = 2;
    let text_width = width.saturating_sub(icon_col);

    let end = items.len().min(scroll + visible_height);
    let mut lines = Vec::new();
    for i in scroll..end {
        let dep = &items[i];
        let is_selected = cursor == Some(i);

        // Determine diff tag for deps
        let diff_tag = diff_dep_tags.and_then(|tags| tags.get(i).copied());

        let bg = if is_selected {
            match diff_tag {
                Some(DiffTag::Removed) => DIFF_DELETE_CURSOR_BG,
                Some(DiffTag::Added) => DIFF_INSERT_CURSOR_BG,
                Some(DiffTag::Modified) => DIFF_MODIFIED_CURSOR_BG,
                _ => CURSOR_BG,
            }
        } else {
            match diff_tag {
                Some(DiffTag::Removed) => DIFF_DELETE_BG,
                Some(DiffTag::Added) => DIFF_INSERT_BG,
                Some(DiffTag::Modified) => DIFF_MODIFIED_BG,
                _ => Color::Reset,
            }
        };

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

/// Compute per-dep diff tags by comparing old and new dep lists.
pub(super) fn compute_dep_diff_tags(
    items: &[String],
    old_items: Option<&Vec<String>>,
    new_items: Option<&Vec<String>>,
) -> Vec<DiffTag> {
    items
        .iter()
        .map(|dep| {
            let in_old = old_items.map(|o| o.contains(dep)).unwrap_or(false);
            let in_new = new_items.map(|n| n.contains(dep)).unwrap_or(false);
            match (in_old, in_new) {
                (true, false) => DiffTag::Removed,
                (false, true) => DiffTag::Added,
                _ => DiffTag::Unchanged,
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Non-TTY text output
// ---------------------------------------------------------------------------

pub(super) fn print_tree_text(children: &[(String, ConfigNode)], depth: usize) {
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

pub(super) const HELP_LINES: &[(&str, &str)] = &[
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
    ("Tab", "Cycle through panes"),
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

pub(super) const DIFF_HELP_LINES: &[(&str, &str)] = &[
    ("", ""),
    ("Tree Diff", ""),
    ("t", "Cycle filter: all / changed / value changed"),
    ("Enter on modified leaf", "Side-by-side diff pager"),
    ("", ""),
    ("Panel Navigation", ""),
    ("Tab", "Cycle focus: Browse \u{2192} Detail \u{2192} Deps \u{2192} Revs"),
];
