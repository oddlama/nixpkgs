use anyhow::Result;
use crossterm::event::KeyCode;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::json2nix;
use crate::resolve;
use crate::tui;

pub fn run(config: &str, explicit: bool, flat: bool, nix_args: &[String]) -> Result<()> {
    let json = resolve::resolve(config, explicit, nix_args)?;
    let nix_text = json2nix::convert(&json, flat);

    if tui::is_tty() {
        run_tui(&nix_text, config)
    } else {
        print!("{}", nix_text);
        Ok(())
    }
}

/// TUI full-screen scrollable viewer.
fn run_tui(text: &str, label: &str) -> Result<()> {
    let content_lines: Vec<&str> = text.lines().collect();
    let total = content_lines.len();

    let mut terminal = tui::setup()?;
    let mut scroll: usize = 0;
    let mut mode = Mode::Normal;
    let mut search_query = String::new();
    let mut search_matches: Vec<usize> = Vec::new();
    let mut search_idx: usize = 0;
    let mut save_path = String::new();
    let mut status_msg: Option<String> = None;

    loop {
        terminal.draw(|frame| {
            let size = frame.area();
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(1), // header
                    Constraint::Min(1),    // content
                    Constraint::Length(1), // footer/input
                ])
                .split(size);

            // Header
            let header = Line::from(vec![
                Span::styled(
                    format!(" {} ", label),
                    Style::default().add_modifier(Modifier::BOLD),
                ),
                Span::raw(format!("  ({} lines)", total)),
            ]);
            frame.render_widget(
                Paragraph::new(header).style(Style::default().bg(Color::DarkGray)),
                chunks[0],
            );

            // Content with line numbers
            let visible = chunks[1].height as usize;
            let end = total.min(scroll + visible);
            let gutter_width = format!("{}", total).len();

            let mut lines = Vec::new();
            for i in scroll..end {
                let line_num = format!("{:>width$} ", i + 1, width = gutter_width);
                let is_match = search_matches.contains(&i);
                let content_style = if is_match {
                    Style::default().bg(Color::Yellow).fg(Color::Black)
                } else {
                    Style::default()
                };
                lines.push(Line::from(vec![
                    Span::styled(line_num, Style::default().fg(Color::DarkGray)),
                    Span::styled(content_lines[i].to_string(), content_style),
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
                            Span::raw(":scroll "),
                            Span::styled("g/G", Style::default().add_modifier(Modifier::BOLD)),
                            Span::raw(":top/bot "),
                            Span::styled("/", Style::default().add_modifier(Modifier::BOLD)),
                            Span::raw(":search "),
                            Span::styled("s", Style::default().add_modifier(Modifier::BOLD)),
                            Span::raw(":save "),
                            Span::styled("q", Style::default().add_modifier(Modifier::BOLD)),
                            Span::raw(":quit"),
                            Span::raw(format!(
                                "  [{}-{}/{}]",
                                scroll + 1,
                                end,
                                total
                            )),
                        ])
                    }
                }
                Mode::Search => Line::from(vec![
                    Span::styled(" /", Style::default().add_modifier(Modifier::BOLD)),
                    Span::raw(&search_query),
                ]),
                Mode::Save => Line::from(vec![
                    Span::styled(
                        " Save to: ",
                        Style::default().add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(&save_path),
                ]),
            };
            frame.render_widget(
                Paragraph::new(footer_line).style(Style::default().bg(Color::DarkGray)),
                chunks[2],
            );
        })?;

        let key = tui::read_key()?;
        let visible = terminal.size()?.height.saturating_sub(2) as usize;

        match &mode {
            Mode::Normal => {
                status_msg = None;
                if tui::is_quit(&key) {
                    break;
                }
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
                    KeyCode::Char('/') => {
                        mode = Mode::Search;
                        search_query.clear();
                    }
                    KeyCode::Char('n') => {
                        // Next search match
                        if !search_matches.is_empty() {
                            search_idx = (search_idx + 1) % search_matches.len();
                            scroll = search_matches[search_idx];
                        }
                    }
                    KeyCode::Char('N') => {
                        // Previous search match
                        if !search_matches.is_empty() {
                            search_idx = search_idx
                                .checked_sub(1)
                                .unwrap_or(search_matches.len() - 1);
                            scroll = search_matches[search_idx];
                        }
                    }
                    KeyCode::Char('s') => {
                        mode = Mode::Save;
                        save_path.clear();
                    }
                    _ => {}
                }
            }
            Mode::Search => match key.code {
                KeyCode::Enter => {
                    search_matches = content_lines
                        .iter()
                        .enumerate()
                        .filter(|(_, line)| line.contains(search_query.as_str()))
                        .map(|(i, _)| i)
                        .collect();
                    search_idx = 0;
                    if let Some(&first) = search_matches.first() {
                        scroll = first;
                    }
                    mode = Mode::Normal;
                    if search_matches.is_empty() {
                        status_msg = Some(format!("No matches for '{}'", search_query));
                    } else {
                        status_msg =
                            Some(format!("{} matches", search_matches.len()));
                    }
                }
                KeyCode::Esc => {
                    mode = Mode::Normal;
                    search_matches.clear();
                }
                KeyCode::Backspace => {
                    search_query.pop();
                }
                KeyCode::Char(c) => {
                    search_query.push(c);
                }
                _ => {}
            },
            Mode::Save => match key.code {
                KeyCode::Enter => {
                    let path = save_path.clone();
                    mode = Mode::Normal;
                    if !path.is_empty() {
                        let full_text: String = content_lines.join("\n") + "\n";
                        match std::fs::write(&path, &full_text) {
                            Ok(()) => status_msg = Some(format!("Saved to {}", path)),
                            Err(e) => status_msg = Some(format!("Error: {}", e)),
                        }
                    }
                }
                KeyCode::Esc => {
                    mode = Mode::Normal;
                }
                KeyCode::Backspace => {
                    save_path.pop();
                }
                KeyCode::Char(c) => {
                    save_path.push(c);
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
    Save,
}
