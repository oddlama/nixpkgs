use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph};
use ratatui::Frame;

use crate::diff;
use crate::theme::*;
use super::types::*;
use super::icons::*;
use super::data::*;
use super::widgets::*;

pub(super) fn render_frame(
    frame: &mut Frame,
    mode: &Mode,
    state: &mut MillerState,
    root_children: &[(String, ConfigNode)],
    deps_index: &DepsIndex,
    config: &str,
    status_msg: &Option<String>,
    pane_areas: &mut PaneAreas,
    diff_ctx: Option<&DiffContext>,
) {
    let middle_children = get_children_at_path(root_children, &state.path);
    let middle_count = middle_children.map(|c| c.len()).unwrap_or(0);

    let size = frame.area();
    let screen_width = size.width;

    match mode {
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
        // DiffPager layout (side-by-side diff viewer)
        // =============================================================
        Mode::DiffPager {
            path,
            diff_lines,
            collapsed_view,
            hunks: _,
            scroll: p_scroll,
            collapsed,
        } => {
            let total = if *collapsed { collapsed_view.len() } else { diff_lines.len() };

            let outer = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(1), // header
                    Constraint::Min(1),    // content
                    Constraint::Length(1), // footer
                ])
                .split(size);

            // Header
            let title = path.join(".");
            let header_area = outer[0];
            frame.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::styled(
                        format!(" {} ", title),
                        Style::default().fg(BLUE).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        " [DIFF]",
                        Style::default().fg(MAGENTA).add_modifier(Modifier::BOLD),
                    ),
                ]))
                .style(Style::default().bg(HEADER_BG)),
                header_area,
            );

            // Content: side-by-side
            let content_area = outer[1];
            let half_width = content_area.width / 2;
            let left_area = Rect {
                x: content_area.x,
                y: content_area.y,
                width: half_width,
                height: content_area.height,
            };
            let right_area = Rect {
                x: content_area.x + half_width,
                y: content_area.y,
                width: content_area.width - half_width,
                height: content_area.height,
            };

            let visible = content_area.height as usize;
            let sc = (*p_scroll).min(total.saturating_sub(1));
            let end = total.min(sc + visible);

            let mut left_lines = Vec::new();
            let mut right_lines = Vec::new();

            let render_diff_line = |dl: &diff::DiffLine, left_lines: &mut Vec<Line>, right_lines: &mut Vec<Line>| {
                let (left_style, right_style) = match (&dl.left, &dl.right) {
                    (Some(l), Some(r)) if l != r => (
                        Style::default().bg(DIFF_DELETE_BG),
                        Style::default().bg(DIFF_INSERT_BG),
                    ),
                    (Some(_), None) => (
                        Style::default().bg(DIFF_DELETE_BG),
                        Style::default(),
                    ),
                    (None, Some(_)) => (
                        Style::default(),
                        Style::default().bg(DIFF_INSERT_BG),
                    ),
                    _ => (Style::default(), Style::default()),
                };

                left_lines.push(Line::from(Span::styled(
                    dl.left.as_deref().unwrap_or("").to_string(),
                    left_style,
                )));
                right_lines.push(Line::from(Span::styled(
                    dl.right.as_deref().unwrap_or("").to_string(),
                    right_style,
                )));
            };

            if *collapsed {
                for disp in &collapsed_view[sc..end] {
                    match disp {
                        diff::DisplayLine::Real(idx) => {
                            render_diff_line(&diff_lines[*idx], &mut left_lines, &mut right_lines);
                        }
                        diff::DisplayLine::Separator(count) => {
                            let sep_style = Style::default()
                                .fg(COMMENT)
                                .add_modifier(Modifier::DIM);
                            let text = format!("--- {} lines hidden ---", count);
                            left_lines.push(Line::from(Span::styled(text.clone(), sep_style)));
                            right_lines.push(Line::from(Span::styled(text, sep_style)));
                        }
                    }
                }
            } else {
                for dl in &diff_lines[sc..end] {
                    render_diff_line(dl, &mut left_lines, &mut right_lines);
                }
            }

            let left_block = Block::default().borders(Borders::RIGHT);
            let left_para = Paragraph::new(left_lines).block(left_block);
            let right_para = Paragraph::new(right_lines);

            frame.render_widget(left_para, left_area);
            frame.render_widget(right_para, right_area);

            // Footer
            let collapse_label = if *collapsed { "expand" } else { "collapse" };
            let mut footer_spans: Vec<Span> = vec![Span::raw(" ")];
            footer_spans.extend(footer_pill("j/k", "scroll"));
            footer_spans.extend(footer_pill("n/N", "hunk"));
            footer_spans.extend(footer_pill("g/G", "top/bot"));
            footer_spans.extend(footer_pill("e", collapse_label));
            footer_spans.extend(footer_pill("q", "back"));
            footer_spans.push(Span::styled(
                format!("[{}-{}/{}]", sc + 1, end, total),
                Style::default().fg(COMMENT),
            ));
            frame.render_widget(
                Paragraph::new(Line::from(footer_spans))
                    .style(Style::default().bg(HEADER_BG)),
                outer[2],
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
                let node = lookup_node(root_children, &results[i]);
                result_lines.push(render_search_result_line(
                    &results[i],
                    query,
                    i == cu,
                    results_width,
                    node,
                    diff_ctx,
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
                    get_node_at_path(root_children, parent, name)
                        .map(|node| (rp.as_slice(), node))
                } else {
                    None
                };

            let path_str = selected_info
                .map(|(rp, _)| rp.join("."))
                .unwrap_or_default();
            let all_dep_items: Vec<String> = deps_index
                .dependencies
                .get(&path_str)
                .cloned()
                .unwrap_or_default();
            let all_rev_items: Vec<String> = deps_index
                .dependents
                .get(&path_str)
                .cloned()
                .unwrap_or_default();

            let s_detail_active = *right_focus == Focus::Detail;
            let s_deps_active = *right_focus == Focus::Deps;
            let s_revs_active = *right_focus == Focus::Revs;

            // Compute diff tags and filter in diff modes
            let s_should_filter = diff_ctx.map(|ctx| ctx.filter != DiffFilter::All).unwrap_or(false);
            let (dep_items, dep_diff_tags) = if let Some(ctx) = diff_ctx {
                let tags = compute_dep_diff_tags(
                    &all_dep_items,
                    ctx.old_deps.dependencies.get(&path_str),
                    ctx.new_deps.dependencies.get(&path_str),
                );
                if s_should_filter {
                    let (fi, ft): (Vec<_>, Vec<_>) = all_dep_items.into_iter()
                        .zip(tags.into_iter())
                        .filter(|(_, t)| *t != DiffTag::Unchanged)
                        .unzip();
                    (fi, Some(ft))
                } else {
                    (all_dep_items, Some(tags))
                }
            } else {
                (all_dep_items, None)
            };
            let (rev_items, rev_diff_tags) = if let Some(ctx) = diff_ctx {
                let tags = compute_dep_diff_tags(
                    &all_rev_items,
                    ctx.old_deps.dependents.get(&path_str),
                    ctx.new_deps.dependents.get(&path_str),
                );
                if s_should_filter {
                    let (fi, ft): (Vec<_>, Vec<_>) = all_rev_items.into_iter()
                        .zip(tags.into_iter())
                        .filter(|(_, t)| *t != DiffTag::Unchanged)
                        .unzip();
                    (fi, Some(ft))
                } else {
                    (all_rev_items, Some(tags))
                }
            } else {
                (all_rev_items, None)
            };
            let dep_count = dep_items.len();
            let rev_count = rev_items.len();

            // Detail pane
            let detail_block = make_block_keyed("Detail", None, 'd', s_detail_active);
            let detail_inner = detail_block.inner(right_stack[0]);
            let detail_height = detail_inner.height as usize;
            let detail_width = detail_inner.width;
            let detail_lines = if let Some((rp, node)) = selected_info {
                if let Some(ctx) = diff_ctx {
                    render_diff_detail_info(rp, node, ctx, *s_detail_scroll, detail_width, detail_height)
                } else {
                    render_detail_info(rp, node, *s_detail_scroll, detail_width, detail_height)
                }
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
            let deps_lines = render_dep_list(
                &dep_items, deps_cursor_val, deps_scroll_val, deps_height,
                deps_inner.width, root_children,
                dep_diff_tags.as_deref(),
            );
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
            let rev_lines = render_dep_list(
                &rev_items, revs_cursor_val, revs_scroll_val, rev_height,
                rev_inner.width, root_children,
                rev_diff_tags.as_deref(),
            );
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
                    header_spans.push(Span::styled(
                        seg.clone(),
                        Style::default()
                            .fg(BLUE)
                            .add_modifier(Modifier::BOLD),
                    ));
                }
            }
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
                    get_children_at_path(root_children, parent_path)
                {
                    let highlight = state.path.last().map(|s| s.as_str());
                    let highlight_idx = highlight.and_then(|h| {
                        parent_children.iter().position(|(n, _)| n == h)
                    });
                    let left_scroll = highlight_idx
                        .map(|idx| idx.saturating_sub(left_height / 2))
                        .unwrap_or(0);
                    let parent_prefix: Vec<String> = parent_path.to_vec();
                    render_pane_list(
                        parent_children,
                        highlight,
                        None,
                        left_scroll,
                        left_height,
                        left_width,
                        diff_ctx,
                        &parent_prefix,
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
                    diff_ctx,
                    &state.path.clone(),
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
                if let Some((name, ConfigNode::Branch(ch))) = selected {
                    let mut child_prefix = state.path.clone();
                    child_prefix.push(name.to_string());
                    render_pane_list(
                        ch,
                        None,
                        None,
                        0,
                        children_height,
                        children_width,
                        diff_ctx,
                        &child_prefix,
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

            let all_dep_items: Vec<String> = deps_index
                .dependencies
                .get(&path_str)
                .cloned()
                .unwrap_or_default();
            let all_rev_items: Vec<String> = deps_index
                .dependents
                .get(&path_str)
                .cloned()
                .unwrap_or_default();

            let detail_active = state.focus == Focus::Detail;
            let deps_active = state.focus == Focus::Deps;
            let revs_active = state.focus == Focus::Revs;

            // Compute diff tags for deps/revs, filtering in diff modes
            let should_filter_deps = diff_ctx.map(|ctx| ctx.filter != DiffFilter::All).unwrap_or(false);
            let (dep_items, dep_diff_tags) = if let Some(ctx) = diff_ctx {
                let tags = compute_dep_diff_tags(
                    &all_dep_items,
                    ctx.old_deps.dependencies.get(&path_str),
                    ctx.new_deps.dependencies.get(&path_str),
                );
                if should_filter_deps {
                    let (fi, ft): (Vec<_>, Vec<_>) = all_dep_items.into_iter()
                        .zip(tags.into_iter())
                        .filter(|(_, t)| *t != DiffTag::Unchanged)
                        .unzip();
                    (fi, Some(ft))
                } else {
                    (all_dep_items, Some(tags))
                }
            } else {
                (all_dep_items, None)
            };
            let (rev_items, rev_diff_tags) = if let Some(ctx) = diff_ctx {
                let tags = compute_dep_diff_tags(
                    &all_rev_items,
                    ctx.old_deps.dependents.get(&path_str),
                    ctx.new_deps.dependents.get(&path_str),
                );
                if should_filter_deps {
                    let (fi, ft): (Vec<_>, Vec<_>) = all_rev_items.into_iter()
                        .zip(tags.into_iter())
                        .filter(|(_, t)| *t != DiffTag::Unchanged)
                        .unzip();
                    (fi, Some(ft))
                } else {
                    (all_rev_items, Some(tags))
                }
            } else {
                (all_rev_items, None)
            };
            let dep_count = dep_items.len();
            let rev_count = rev_items.len();

            let narrow = screen_width < 110;

            if narrow {
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
                    if let Some(ctx) = diff_ctx {
                        render_diff_detail_info(&full_path, node, ctx, state.detail_scroll, detail_width, detail_height)
                    } else {
                        render_detail_info(&full_path, node, state.detail_scroll, detail_width, detail_height)
                    }
                } else {
                    vec![Line::from(""); detail_height]
                };
                frame.render_widget(detail_block, bottom[0]);
                frame.render_widget(Paragraph::new(detail_lines), detail_inner);

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
                let deps_lines = render_dep_list(
                    &dep_items, deps_cursor_val, state.deps_scroll, deps_height,
                    deps_inner.width, root_children,
                    dep_diff_tags.as_deref(),
                );
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
                let rev_lines = render_dep_list(
                    &rev_items, revs_cursor_val, state.deps_scroll, rev_height,
                    rev_inner.width, root_children,
                    rev_diff_tags.as_deref(),
                );
                frame.render_widget(rev_block, right_stack[1]);
                frame.render_widget(Paragraph::new(rev_lines), rev_inner);

                pane_areas.detail_inner = detail_inner;
                pane_areas.deps_inner = deps_inner;
                pane_areas.revs_inner = rev_inner;
                pane_areas.deps_scroll = state.deps_scroll;
                pane_areas.deps_count = dep_count;
                pane_areas.revs_count = rev_count;
            } else {
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
                    if let Some(ctx) = diff_ctx {
                        render_diff_detail_info(&full_path, node, ctx, state.detail_scroll, detail_width, detail_height)
                    } else {
                        render_detail_info(&full_path, node, state.detail_scroll, detail_width, detail_height)
                    }
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
                let deps_lines = render_dep_list(
                    &dep_items, deps_cursor_val, state.deps_scroll, deps_height,
                    deps_inner.width, root_children,
                    dep_diff_tags.as_deref(),
                );
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
                let rev_lines = render_dep_list(
                    &rev_items, revs_cursor_val, state.deps_scroll, rev_height,
                    rev_inner.width, root_children,
                    rev_diff_tags.as_deref(),
                );
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
            let footer_line = if let Some(msg) = status_msg {
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
                spans.extend(footer_pill("Tab", "panes"));
                spans.extend(footer_pill("/", "search"));
                if let Some(ctx) = diff_ctx {
                    let label = format!("filter: {}", ctx.filter.label());
                    spans.extend(footer_pill("t", &label));
                }
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

                let all_lines: Vec<(&str, &str)> = if diff_ctx.is_some() {
                    HELP_LINES.iter().chain(DIFF_HELP_LINES.iter()).copied().collect()
                } else {
                    HELP_LINES.to_vec()
                };

                let mut help_lines: Vec<Line> = Vec::new();
                for (key, desc) in &all_lines {
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
}
