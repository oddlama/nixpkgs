use std::io::Write;
use std::process::Command;

use anyhow::{Context, Result};
use crossterm::event::KeyCode;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use similar::{ChangeTag, TextDiff};

use crate::json2nix;
use crate::resolve;
use crate::tui;

pub fn run(
    old_arg: &str,
    new_arg: &str,
    explicit: bool,
    exec_cmd: Option<&str>,
) -> Result<()> {
    let old_json = resolve::resolve(old_arg, explicit)?;
    let new_json = resolve::resolve(new_arg, explicit)?;
    let old_nix = json2nix::convert(&old_json, true);
    let new_nix = json2nix::convert(&new_json, true);

    if let Some(cmd) = exec_cmd {
        return run_exec(cmd, &old_nix, &new_nix);
    }

    if tui::is_tty() {
        run_tui(&old_nix, &new_nix, old_arg, new_arg)
    } else {
        run_pipe(&old_nix, &new_nix)
    }
}

/// Write both texts to temp files and run an external diff command.
fn run_exec(cmd: &str, old_nix: &str, new_nix: &str) -> Result<()> {
    let mut old_file = tempfile::Builder::new()
        .suffix(".nix")
        .tempfile()
        .context("creating temp file")?;
    let mut new_file = tempfile::Builder::new()
        .suffix(".nix")
        .tempfile()
        .context("creating temp file")?;

    old_file.write_all(old_nix.as_bytes())?;
    new_file.write_all(new_nix.as_bytes())?;

    let old_path = old_file.path().to_str().unwrap();
    let new_path = new_file.path().to_str().unwrap();

    let status = Command::new("sh")
        .args(["-c", &format!("{} {} {}", cmd, old_path, new_path)])
        .status()
        .context("running external diff command")?;

    std::process::exit(status.code().unwrap_or(1));
}

/// Print unified diff to stdout (non-TTY mode).
fn run_pipe(old_nix: &str, new_nix: &str) -> Result<()> {
    let diff = TextDiff::from_lines(old_nix, new_nix);
    let udiff = diff
        .unified_diff()
        .header("a/config.nix", "b/config.nix")
        .to_string();

    if udiff.is_empty() {
        // No differences
        return Ok(());
    }

    print!("{}", udiff);
    // Exit 1 to match diff convention (1 = differences found)
    std::process::exit(1);
}

/// A paired line for side-by-side display.
struct DiffLine {
    left: Option<String>,
    right: Option<String>,
    tag: ChangeTag,
}

/// Build paired lines for side-by-side display.
fn build_diff_lines(old_nix: &str, new_nix: &str) -> Vec<DiffLine> {
    let diff = TextDiff::from_lines(old_nix, new_nix);
    let mut lines = Vec::new();
    let mut removes: Vec<String> = Vec::new();
    let mut inserts: Vec<String> = Vec::new();

    let flush = |lines: &mut Vec<DiffLine>, removes: &mut Vec<String>, inserts: &mut Vec<String>| {
        let max_len = removes.len().max(inserts.len());
        for i in 0..max_len {
            lines.push(DiffLine {
                left: removes.get(i).cloned(),
                right: inserts.get(i).cloned(),
                tag: if removes.get(i).is_some() && inserts.get(i).is_some() {
                    ChangeTag::Equal // "modified" — shown as delete+insert pair
                } else if removes.get(i).is_some() {
                    ChangeTag::Delete
                } else {
                    ChangeTag::Insert
                },
            });
        }
        removes.clear();
        inserts.clear();
    };

    for change in diff.iter_all_changes() {
        let text = change.value().trim_end_matches('\n').to_string();
        match change.tag() {
            ChangeTag::Equal => {
                flush(&mut lines, &mut removes, &mut inserts);
                lines.push(DiffLine {
                    left: Some(text.clone()),
                    right: Some(text),
                    tag: ChangeTag::Equal,
                });
            }
            ChangeTag::Delete => removes.push(text),
            ChangeTag::Insert => inserts.push(text),
        }
    }
    flush(&mut lines, &mut removes, &mut inserts);
    lines
}

/// Find hunk start positions (lines where changes begin after equal lines).
fn find_hunks(lines: &[DiffLine]) -> Vec<usize> {
    let mut hunks = Vec::new();
    let mut in_change = false;
    for (i, line) in lines.iter().enumerate() {
        let is_change = !matches!(line.tag, ChangeTag::Equal)
            || (line.left.is_some() != line.right.is_some());
        let is_change = is_change
            || (line.left.as_deref() != line.right.as_deref());
        if is_change && !in_change {
            hunks.push(i);
            in_change = true;
        } else if !is_change {
            in_change = false;
        }
    }
    hunks
}

const CONTEXT_LINES: usize = 3;

/// A display line in collapsed or full view.
enum DisplayLine {
    /// A real diff line, with its index into the original diff_lines vec.
    Real(usize),
    /// A separator representing hidden equal lines; stores the count hidden.
    Separator(usize),
}

/// Build a collapsed view showing only change regions + context lines around them.
fn build_collapsed_view(diff_lines: &[DiffLine]) -> Vec<DisplayLine> {
    let total = diff_lines.len();
    if total == 0 {
        return Vec::new();
    }

    // Mark which original lines are visible (changed or within CONTEXT_LINES of a change)
    let mut visible = vec![false; total];
    for (i, dl) in diff_lines.iter().enumerate() {
        let is_change = dl.left.as_deref() != dl.right.as_deref();
        if is_change {
            let start = i.saturating_sub(CONTEXT_LINES);
            let end = (i + CONTEXT_LINES + 1).min(total);
            for v in &mut visible[start..end] {
                *v = true;
            }
        }
    }

    let mut result = Vec::new();
    let mut i = 0;
    while i < total {
        if visible[i] {
            result.push(DisplayLine::Real(i));
            i += 1;
        } else {
            // Count consecutive hidden lines
            let start = i;
            while i < total && !visible[i] {
                i += 1;
            }
            result.push(DisplayLine::Separator(i - start));
        }
    }
    result
}

/// TUI side-by-side diff viewer.
fn run_tui(old_nix: &str, new_nix: &str, old_label: &str, new_label: &str) -> Result<()> {
    let diff_lines = build_diff_lines(old_nix, new_nix);
    let hunks = find_hunks(&diff_lines);

    if diff_lines.iter().all(|l| l.left == l.right) {
        eprintln!("No differences.");
        return Ok(());
    }

    let collapsed_view = build_collapsed_view(&diff_lines);

    let mut terminal = tui::setup()?;
    let mut scroll: usize = 0;
    let mut collapsed = true;

    loop {
        let total = if collapsed {
            collapsed_view.len()
        } else {
            diff_lines.len()
        };

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

            // Header: split into left/right halves matching content columns
            let header_area = chunks[0];
            let header_half = header_area.width / 2;
            let header_left = Rect {
                x: header_area.x,
                y: header_area.y,
                width: header_half,
                height: header_area.height,
            };
            let header_right = Rect {
                x: header_area.x + header_half,
                y: header_area.y,
                width: header_area.width - header_half,
                height: header_area.height,
            };
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    format!(" {} ", old_label),
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                )))
                .style(Style::default().bg(Color::DarkGray)),
                header_left,
            );
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    format!(" {} ", new_label),
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                )))
                .style(Style::default().bg(Color::DarkGray)),
                header_right,
            );

            // Content: side-by-side
            let content_area = chunks[1];
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
            let end = total.min(scroll + visible);

            let mut left_lines = Vec::new();
            let mut right_lines = Vec::new();

            let render_diff_line = |dl: &DiffLine, left_lines: &mut Vec<Line>, right_lines: &mut Vec<Line>| {
                let (left_style, right_style) = match (&dl.left, &dl.right) {
                    (Some(l), Some(r)) if l != r => (
                        Style::default().bg(Color::Rgb(80, 0, 0)),
                        Style::default().bg(Color::Rgb(0, 60, 0)),
                    ),
                    (Some(_), None) => (
                        Style::default().bg(Color::Rgb(80, 0, 0)),
                        Style::default(),
                    ),
                    (None, Some(_)) => (
                        Style::default(),
                        Style::default().bg(Color::Rgb(0, 60, 0)),
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

            if collapsed {
                for disp in &collapsed_view[scroll..end] {
                    match disp {
                        DisplayLine::Real(idx) => {
                            render_diff_line(&diff_lines[*idx], &mut left_lines, &mut right_lines);
                        }
                        DisplayLine::Separator(count) => {
                            let sep_style = Style::default()
                                .fg(Color::DarkGray)
                                .add_modifier(Modifier::DIM);
                            let text = format!("--- {} lines hidden ---", count);
                            left_lines.push(Line::from(Span::styled(text.clone(), sep_style)));
                            right_lines.push(Line::from(Span::styled(text, sep_style)));
                        }
                    }
                }
            } else {
                for dl in &diff_lines[scroll..end] {
                    render_diff_line(dl, &mut left_lines, &mut right_lines);
                }
            }

            let left_block = Block::default().borders(Borders::RIGHT);
            let left_para = Paragraph::new(left_lines).block(left_block);
            let right_para = Paragraph::new(right_lines);

            frame.render_widget(left_para, left_area);
            frame.render_widget(right_para, right_area);

            // Footer
            let collapse_label = if collapsed { "expand" } else { "collapse" };
            let footer = Line::from(vec![
                Span::styled(
                    " j/k",
                    Style::default().add_modifier(Modifier::BOLD),
                ),
                Span::raw(":scroll "),
                Span::styled("n/N", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(":hunk "),
                Span::styled("g/G", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(":top/bot "),
                Span::styled("e", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(format!(":{} ", collapse_label)),
                Span::styled("q", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(":quit"),
                Span::raw(format!(
                    "  [{}-{}/{}]",
                    scroll + 1,
                    end,
                    total
                )),
            ]);
            frame.render_widget(
                Paragraph::new(footer).style(Style::default().bg(Color::DarkGray)),
                chunks[2],
            );
        })?;

        let key = tui::read_key()?;
        if tui::is_quit(&key) {
            break;
        }

        let visible = terminal.size()?.height.saturating_sub(2) as usize;
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => {
                if scroll + 1 < total {
                    scroll += 1;
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                scroll = scroll.saturating_sub(1);
            }
            KeyCode::PageDown => {
                scroll = total.saturating_sub(1).min(scroll + visible);
            }
            KeyCode::PageUp => {
                scroll = scroll.saturating_sub(visible);
            }
            KeyCode::Char('g') | KeyCode::Home => {
                scroll = 0;
            }
            KeyCode::Char('G') | KeyCode::End => {
                scroll = total.saturating_sub(visible);
            }
            KeyCode::Char('e') => {
                collapsed = !collapsed;
                scroll = if collapsed {
                    // Find the collapsed-view index closest to current scroll position
                    // by looking for the first Real entry at or after the current scroll
                    collapsed_view
                        .iter()
                        .position(|d| matches!(d, DisplayLine::Real(idx) if *idx >= scroll))
                        .unwrap_or(0)
                } else {
                    // Expanding: find the original index of the current collapsed line
                    match collapsed_view.get(scroll) {
                        Some(DisplayLine::Real(idx)) => *idx,
                        _ => 0,
                    }
                };
            }
            KeyCode::Char('n') => {
                if collapsed {
                    // In collapsed mode, find next hunk by display index
                    if let Some(pos) = collapsed_view[scroll.saturating_add(1)..]
                        .iter()
                        .position(|d| {
                            matches!(d, DisplayLine::Real(idx) if {
                                let dl = &diff_lines[*idx];
                                dl.left.as_deref() != dl.right.as_deref()
                            })
                        })
                    {
                        // Find the start of this hunk's change region
                        let abs_pos = scroll + 1 + pos;
                        // Walk back to find where this hunk starts
                        let mut hunk_start = abs_pos;
                        while hunk_start > 0 {
                            if let DisplayLine::Real(idx) = &collapsed_view[hunk_start - 1] {
                                if diff_lines[*idx].left.as_deref() != diff_lines[*idx].right.as_deref() {
                                    hunk_start -= 1;
                                    continue;
                                }
                            }
                            break;
                        }
                        // Only advance if this is a different hunk than current position
                        if hunk_start > scroll {
                            scroll = hunk_start;
                        } else {
                            scroll = abs_pos;
                        }
                    }
                } else {
                    if let Some(&h) = hunks.iter().find(|&&h| h > scroll) {
                        scroll = h;
                    }
                }
            }
            KeyCode::Char('N') => {
                if collapsed {
                    // In collapsed mode, find previous change line
                    if scroll > 0 {
                        if let Some(pos) = collapsed_view[..scroll]
                            .iter()
                            .rposition(|d| {
                                matches!(d, DisplayLine::Real(idx) if {
                                    let dl = &diff_lines[*idx];
                                    dl.left.as_deref() != dl.right.as_deref()
                                })
                            })
                        {
                            // Walk back to start of this hunk
                            let mut hunk_start = pos;
                            while hunk_start > 0 {
                                if let DisplayLine::Real(idx) = &collapsed_view[hunk_start - 1] {
                                    if diff_lines[*idx].left.as_deref() != diff_lines[*idx].right.as_deref() {
                                        hunk_start -= 1;
                                        continue;
                                    }
                                }
                                break;
                            }
                            scroll = hunk_start;
                        }
                    }
                } else {
                    if let Some(&h) = hunks.iter().rev().find(|&&h| h < scroll) {
                        scroll = h;
                    }
                }
            }
            _ => {}
        }
    }

    tui::teardown(terminal)?;
    Ok(())
}
