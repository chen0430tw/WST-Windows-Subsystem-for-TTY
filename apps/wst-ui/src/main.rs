mod builtin;

use anyhow::Result;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame, Terminal,
};
use std::io;
use std::time::Duration;
use wst_config::WstConfig;
use wst_core::WstCore;
use wst_protocol::{BackendKind, SessionEvent, TaskStatus};

const INPUT_PROMPT: &str = ">";
const VERSION: &str = env!("CARGO_PKG_VERSION");

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
            output: vec![],
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

        // Add command to output as if it was typed (like real terminal)
        self.output.push(OutputLine::normal(format!("{} {}", INPUT_PROMPT, command)));

        // Check for builtin commands
        if command.starts_with(':') {
            self.handle_builtin(&command);
        } else {
            match self.ensure_session() {
                Ok(session) => match self.core.exec_with_session(session, command) {
                    Ok(_task_id) => {
                        // Task started, will poll for output
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
                self.output.push(OutputLine::system(format!("WST v{} - Windows Subsystem for TTY", VERSION)));
                self.output.push(OutputLine::normal(format!("Backend: {:?}", self.core.default_backend())));
                self.output
                    .push(OutputLine::normal(format!("Session: {:?}", self.session_id)));
                self.output
                    .push(OutputLine::normal(format!("History: {} commands", self.core.history().len())));
            }
            ":clear" => {
                self.output.clear();
                self.scroll_offset = 0;
            }
            ":history" => {
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
                self.output.push(OutputLine::normal("Type :help for available commands"));
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
                            // Silent
                        }
                        SessionEvent::Output(chunk) => {
                            if chunk.is_stderr {
                                self.output.push(OutputLine::error(chunk.text));
                            } else {
                                self.output.push(OutputLine::normal(chunk.text));
                            }
                        }
                        SessionEvent::TaskUpdated { task_id, status } => {
                            // Only show errors
                            if let TaskStatus::Exited(code) = status {
                                if code != 0 {
                                    self.output.push(OutputLine::system(format!(
                                        "Process exited with code {}",
                                        code
                                    )));
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    fn scroll_to_bottom(&mut self) {
        self.scroll_offset = self.output.len().saturating_sub(1);
        if self.scroll_offset > 0 {
            self.scroll_offset = self.output.len().saturating_sub(10);
        }
    }
}

fn draw_ui(f: &mut Frame, state: &mut AppState) {
    let size = f.size();

    // Terminal style: full area for output + input
    let main_area = size;

    // Calculate how many lines we can show
    let visible_lines = (main_area.height as usize).saturating_sub(2); // Leave space for input

    // Build terminal content (output lines + current input)
    let mut terminal_lines: Vec<Line> = Vec::new();

    // Add output lines (from scroll offset)
    for line in state.output.iter().skip(state.scroll_offset) {
        let style = if line.is_error {
            Style::default().fg(Color::Red)
        } else if line.is_system {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default().fg(Color::Reset)
        };
        terminal_lines.push(Line::from(vec![Span::styled(&line.text, style)]));
    }

    // Add current input line
    let input_line = vec![
        Span::styled(INPUT_PROMPT, Style::default().fg(Color::Green)),
        Span::raw(" "),
        Span::raw(&state.input),
    ];
    terminal_lines.push(Line::from(input_line));

    // Render as a single paragraph (like a real terminal)
    let terminal = Paragraph::new(terminal_lines)
        .style(Style::default().bg(Color::Black).fg(Color::White));
    f.render_widget(terminal, main_area);
}

fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    mut state: AppState,
) -> Result<()> {
    let mut last_tick = std::time::Instant::now();
    let tick_rate = Duration::from_millis(100);

    while state.running {
        terminal.draw(|f| draw_ui(f, &mut state))?;

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

        if last_tick.elapsed() >= tick_rate {
            state.tick();
            last_tick = std::time::Instant::now();
        }
    }

    Ok(())
}

fn main() -> Result<()> {
    let config = WstConfig::load_default()?;

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let state = AppState::new(config)?;

    let result = run_app(&mut terminal, state);

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
