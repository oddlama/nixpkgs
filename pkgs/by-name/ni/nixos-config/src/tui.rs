use std::io::{self, Stdout};

use anyhow::Result;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyModifiers, MouseEvent},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

pub type Term = Terminal<CrosstermBackend<Stdout>>;

pub enum InputEvent {
    Key(KeyEvent),
    Mouse(MouseEvent),
}

/// Set up the terminal for TUI mode.
pub fn setup() -> Result<Term> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let terminal = Terminal::new(backend)?;
    Ok(terminal)
}

/// Restore the terminal to normal mode.
pub fn teardown(mut terminal: Term) -> Result<()> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableMouseCapture)?;
    terminal.show_cursor()?;
    Ok(())
}

/// Read the next key event, blocking.
pub fn read_key() -> Result<KeyEvent> {
    loop {
        if let Event::Key(key) = event::read()? {
            return Ok(key);
        }
    }
}

/// Read the next key or mouse event, blocking.
pub fn read_input() -> Result<InputEvent> {
    loop {
        match event::read()? {
            Event::Key(key) => return Ok(InputEvent::Key(key)),
            Event::Mouse(mouse) => return Ok(InputEvent::Mouse(mouse)),
            _ => {}
        }
    }
}

/// Check if a key event is a quit signal (q or Ctrl-C).
pub fn is_quit(key: &KeyEvent) -> bool {
    key.code == KeyCode::Char('q')
        || (key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL))
}

/// Returns true if stdout is a TTY.
pub fn is_tty() -> bool {
    use std::io::IsTerminal;
    io::stdout().is_terminal()
}
