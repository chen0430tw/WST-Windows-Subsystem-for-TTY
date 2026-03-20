mod builtin;

use anyhow::Result;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
    Frame, Terminal,
};
use std::io;
use std::time::Duration;
use wst_config::WstConfig;
use wst_core::WstCore;
use wst_protocol::{BackendKind, SessionEvent};

const INPUT_PROMPT: &str = ">";
const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Clone, Copy, PartialEq)]
enum Focus {
    Input,
    Output,
}

struct OutputLine {
    text: String,
    is_error: bool,
    is_system: bool,
}

impl OutputLine {
    fn normal(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            is_error: false,
            is_system: false,
        }
    }

    fn error(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            is_error: true,
            is_system: false,
        }
    }

    fn system(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            is_error: false,
            is_system: true,
        }
    }
}

struct AppState {
    core: WstCore,
    input: String,
    cursor_position: usize,
    output: Vec<OutputLine>,
    focus: Focus,
    running: bool,
    session_id: Option<u64>,
    scroll_offset: usize,
}

impl AppState {
    fn new(config: WstConfig) -> Result<Self> {
        let core = WstCore::new(config);
        Ok(Self {
            core,
            input: String::new(),
            cursor_position: 0,
            output: vec![OutputLine::system(format!(
                "WST v{} - Windows Subsystem for TTY",
                VERSION
            ))],
            focus: Focus::Input,
            running: true,
            session_id: None,
            scroll_offset: 0,
        })
    }

    fn handle_input(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Enter => {
                self.execute_command();
            }
            KeyCode::Char(c) => {
                self.input.insert(self.cursor_position, c);
                self.move_cursor_right();
            }
            KeyCode::Backspace => {
                if self.cursor_position > 0 {
                    self.input.remove(self.cursor_position - 1);
                    self.move_cursor_left();
                }
            }
            KeyCode::Delete => {
                if self.cursor_position < self.input.len() {
                    self.input.remove(self.cursor_position);
                }
            }
            KeyCode::Left => self.move_cursor_left(),
            KeyCode::Right => self.move_cursor_right(),
            KeyCode::Home => {
                self.cursor_position = 0;
            }
            KeyCode::End => {
                self.cursor_position = self.input.len();
            }
            KeyCode::Up => {
                if let Some(cmd) = self.core.history_mut().prev() {
                    self.input = cmd.to_string();
                    self.cursor_position = self.input.len();
                }
            }
            KeyCode::Down => {
                if let Some(cmd) = self.core.history_mut().next() {
                    self.input = cmd.to_string();
                    self.cursor_position = self.input.len();
                } else {
                    self.input.clear();
                    self.cursor_position = 0;
                }
            }
            KeyCode::Esc => {
                self.input.clear();
                self.cursor_position = 0;
                self.core.history_mut().reset();
            }
            _ => {}
        }
    }

    fn move_cursor_left(&mut self) {
        self.cursor_position = self.cursor_position.saturating_sub(1);
    }

    fn move_cursor_right(&mut self) {
        if self.cursor_position < self.input.len() {
            self.cursor_position += 1;
        }
    }

    fn execute_command(&mut self) {
        let command = self.input.trim().to_string();

        if command.is_empty() {
            self.input.clear();
            self.cursor_position = 0;
            return;
        }

        // Check for builtin commands
        if command.starts_with(':') {
            self.handle_builtin(&command);
        } else {
            // Execute via backend
            self.output
                .push(OutputLine::normal(format!("{} {}", INPUT_PROMPT, command)));

            match self.ensure_session() {
                Ok(session) => match self.core.exec_with_session(session, command) {
                    Ok(task_id) => {
                        self.output
                            .push(OutputLine::system(format!("[Task {} started]", task_id)));
                    }
                    Err(e) => {
                        self.output.push(OutputLine::error(format!("Error: {}", e)));
                    }
                },
                Err(e) => {
                    self.output.push(OutputLine::error(format!("Session error: {}", e)));
                }
            }
        }

        self.input.clear();
        self.cursor_position = 0;
        self.scroll_to_bottom();
    }

    fn handle_builtin(&mut self, command: &str) {
        let parts: Vec<&str> = command.split_whitespace().collect();
        let cmd = parts.first().map(|s| *s).unwrap_or(":");

        match cmd {
            ":help" => {
                self.output.push(OutputLine::system("Builtin commands:"));
                self.output.push(OutputLine::normal("  :help        - Show this help"));
                self.output.push(OutputLine::normal("  :status      - Show current status"));
                self.output.push(OutputLine::normal("  :clear       - Clear output"));
                self.output.push(OutputLine::normal("  :history     - Show command history"));
                self.output
                    .push(OutputLine::normal("  :backend     - Switch backend (Cygctl|Pwsh|Cmd)"));
                self.output.push(OutputLine::normal("  :exit / :q   - Exit WST"));
            }
            ":status" => {
                self.output.push(OutputLine::system("WST Status:"));
                self.output.push(OutputLine::normal(format!("  Version: {}", VERSION)));
                self.output.push(OutputLine::normal(format!(
                    "  Backend: {:?}",
                    self.core.default_backend()
                )));
                self.output
                    .push(OutputLine::normal(format!("  Session: {:?}", self.session_id)));
                self.output.push(OutputLine::normal(format!(
                    "  History: {} commands",
                    self.core.history().len()
                )));
                self.output.push(OutputLine::normal(format!(
                    "  Fullscreen: {}",
                    self.core.config().fullscreen
                )));
                self.output
                    .push(OutputLine::normal(format!("  Hotkey: {}", self.core.config().hotkey)));
            }
            ":clear" => {
                self.output.clear();
                self.scroll_offset = 0;
            }
            ":history" => {
                self.output.push(OutputLine::system("Command History:"));
                for (i, entry) in self.core.history().iter().enumerate() {
                    self.output
                        .push(OutputLine::normal(format!("  {}: {}", i + 1, entry.command)));
                }
            }
            ":backend" => {
                if parts.len() < 2 {
                    self.output.push(OutputLine::normal(format!(
                        "Current backend: {:?}",
                        self.core.default_backend()
                    )));
                    self.output
                        .push(OutputLine::normal("Usage: :backend <Cygctl|Pwsh|Cmd>"));
                } else {
                    let new_backend = match parts[1].to_lowercase().as_str() {
                        "cygctl" => Some(BackendKind::Cygctl),
                        "pwsh" => Some(BackendKind::Pwsh),
                        "cmd" => Some(BackendKind::Cmd),
                        _ => None,
                    };

                    if let Some(kind) = new_backend {
                        match self.core.switch_backend(kind) {
                            Ok(()) => {
                                self.output.push(OutputLine::system(format!(
                                    "Switched to {:?} backend",
                                    kind
                                )));
                                self.session_id = None;
                            }
                            Err(e) => {
                                self.output.push(OutputLine::error(format!("Failed to switch: {}", e)));
                            }
                        }
                    } else {
                        self.output
                            .push(OutputLine::error("Unknown backend. Use: Cygctl, Pwsh, or Cmd"));
                    }
                }
            }
            ":exit" | ":q" => {
                self.running = false;
            }
            _ => {
                self.output.push(OutputLine::error(format!("Unknown builtin: {}", cmd)));
                self.output
                    .push(OutputLine::normal("Type :help for available commands"));
            }
        }
    }

    fn ensure_session(&mut self) -> Result<u64> {
        if let Some(id) = self.session_id {
            Ok(id)
        } else {
            let id = self.core.create_session()?;
            self.session_id = Some(id);
            Ok(id)
        }
    }

    fn tick(&mut self) {
        if let Some(session) = self.session_id {
            if let Ok(events) = self.core.tick_session(session) {
                for event in events {
                    match event {
                        SessionEvent::SessionStarted(id) => {
                            self.output
                                .push(OutputLine::system(format!("Session {} started", id)));
                        }
                        SessionEvent::Output(chunk) => {
                            if chunk.is_stderr {
                                self.output.push(OutputLine::error(chunk.text));
                            } else {
                                self.output.push(OutputLine::normal(chunk.text));
                            }
                        }
                        SessionEvent::TaskUpdated { task_id, status } => {
                            self.output.push(OutputLine::system(format!(
                                "Task {} {:?}",
                                task_id, status
                            )));
                        }
                    }
                }
            }
        }
    }

    fn scroll_to_bottom(&mut self) {
        // Will be calculated based on output area size
        self.scroll_offset = self.output.len().saturating_sub(1);
    }
}

fn draw_ui(f: &mut Frame, state: &mut AppState) {
    let size = f.size();

    // Main layout
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints(
            [
                Constraint::Min(0),
                Constraint::Length(3),
                Constraint::Length(1),
            ]
            .as_ref(),
        )
        .split(size);

    let output_area = chunks[0];
    let input_area = chunks[1];
    let status_area = chunks[2];

    // Draw output area
    let output_lines: Vec<ListItem> = state
        .output
        .iter()
        .skip(state.scroll_offset)
        .map(|line| {
            let style = if line.is_error {
                Style::default().fg(Color::Red)
            } else if line.is_system {
                Style::default().fg(Color::Cyan).add_modifier(Modifier::DIM)
            } else {
                Style::default().fg(Color::Reset)
            };
            ListItem::new(line.text.as_str()).style(style)
        })
        .collect();

    let output = List::new(output_lines).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Output "),
    );
    f.render_widget(output, output_area);

    // Draw input area
    let input_text = vec![Line::from(vec![
        Span::styled(INPUT_PROMPT, Style::default().fg(Color::Green)),
        Span::raw(" "),
        Span::raw(&state.input),
        Span::styled(" ", Style::default().fg(Color::DarkGray)), // Cursor placeholder
    ])];

    let input = Paragraph::new(input_text).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Input "),
    );
    f.render_widget(input, input_area);

    // Draw status bar
    let backend_name = format!("{:?}", state.core.default_backend());
    let status_text = vec![Line::from(vec![
        Span::styled(
            format!(" WST {} ", VERSION),
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        ),
        Span::raw(" | "),
        Span::styled(
            format!("Backend: {} ", backend_name),
            Style::default().fg(Color::Yellow),
        ),
        Span::raw(" | "),
        Span::styled(
            format!("Session: {:?} ", state.session_id),
            Style::default().fg(Color::Green),
        ),
        Span::raw(" | "),
        Span::styled(
            format!("History: {} ", state.core.history().len()),
            Style::default().fg(Color::Blue),
        ),
        Span::raw(" | "),
        Span::styled(
            " Ctrl+C to exit ",
            Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC),
        ),
    ])];

    let status = Paragraph::new(status_text)
        .alignment(Alignment::Left)
        .style(Style::default().bg(Color::DarkGray));
    f.render_widget(status, status_area);
}

fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    mut state: AppState,
) -> Result<()> {
    let mut last_tick = std::time::Instant::now();
    let tick_rate = Duration::from_millis(100);

    while state.running {
        // Draw UI
        terminal.draw(|f| draw_ui(f, &mut state))?;

        // Handle events
        let timeout = tick_rate.saturating_sub(last_tick.elapsed());
        if crossterm::event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        state.running = false;
                    }
                    _ => {
                        state.handle_input(key);
                    }
                }
            }
        }

        // Tick for backend events
        if last_tick.elapsed() >= tick_rate {
            state.tick();
            last_tick = std::time::Instant::now();
        }
    }

    Ok(())
}

fn main() -> Result<()> {
    // Load config
    let config = WstConfig::load_default()?;

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Create app state
    let state = AppState::new(config)?;

    // Run app
    let result = run_app(&mut terminal, state);

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;

    result?;

    println!("WST exited. Goodbye!");

    Ok(())
}
