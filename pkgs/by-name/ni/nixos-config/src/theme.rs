use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders};

// ---------------------------------------------------------------------------
// OneHalfDark UI palette
// ---------------------------------------------------------------------------

/// OneHalfDark foreground
pub const FG: Color = Color::Rgb(0xdc, 0xdf, 0xe4);
/// OneHalfDark comment / dim text
pub const COMMENT: Color = Color::Rgb(0x5c, 0x63, 0x70);
/// OneHalfDark gutter / subtle bg accents
pub const GUTTER: Color = Color::Rgb(0x4b, 0x52, 0x63);
/// OneHalfDark blue (active borders, accents)
pub const BLUE: Color = Color::Rgb(0x61, 0xaf, 0xef);
/// OneHalfDark magenta (footer desc bg)
pub const MAGENTA: Color = Color::Rgb(0xc6, 0x78, 0xdd);
/// OneHalfDark green (status messages)
pub const GREEN: Color = Color::Rgb(0x98, 0xc3, 0x79);
/// OneHalfDark red
pub const RED: Color = Color::Rgb(0xe0, 0x6c, 0x75);
/// OneHalfDark yellow
pub const YELLOW: Color = Color::Rgb(0xe5, 0xc0, 0x7b);
/// OneHalfDark cyan
pub const CYAN: Color = Color::Rgb(0x56, 0xb6, 0xc2);

// UI element colors derived from the palette
pub const ACTIVE_BORDER: Color = MAGENTA;
pub const INACTIVE_BORDER: Color = GUTTER;
pub const CURSOR_BG: Color = Color::Rgb(0x2c, 0x31, 0x3c);
pub const PARENT_HIGHLIGHT_BG: Color = Color::Rgb(0x23, 0x27, 0x30);
pub const HIGHLIGHT_BG: Color = Color::Rgb(0x80, 0x60, 0x00);
pub const HEADER_BG: Color = Color::Rgb(0x21, 0x25, 0x2b);
/// Footer key pill background
pub const KEY_BG: Color = Color::Rgb(0x3e, 0x44, 0x52);
/// Footer key pill foreground
pub const KEY_FG: Color = FG;
/// Footer desc pill background
pub const DESC_BG: Color = MAGENTA;
/// Footer desc pill foreground
pub const DESC_FG: Color = Color::Rgb(0x21, 0x25, 0x2b);

// Diff-specific colors
pub const DIFF_DELETE_BG: Color = Color::Rgb(80, 0, 0);
pub const DIFF_INSERT_BG: Color = Color::Rgb(0, 60, 0);
pub const DIFF_MODIFIED_BG: Color = Color::Rgb(0, 30, 80);
// Brighter variants for cursor selection in diff mode
pub const DIFF_DELETE_CURSOR_BG: Color = Color::Rgb(130, 20, 20);
pub const DIFF_INSERT_CURSOR_BG: Color = Color::Rgb(20, 100, 20);
pub const DIFF_MODIFIED_CURSOR_BG: Color = Color::Rgb(20, 50, 130);

// ---------------------------------------------------------------------------
// Widget helpers
// ---------------------------------------------------------------------------

pub fn make_block(title: &str, active: bool) -> Block<'_> {
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

pub fn make_block_keyed<'a>(base: &str, count: Option<usize>, key_char: char, active: bool) -> Block<'a> {
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
pub fn footer_pill<'a>(key: &str, desc: &str) -> Vec<Span<'a>> {
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

pub fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
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
