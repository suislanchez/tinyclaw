use super::{AgentEvent, AgentState};
use crate::providers::UsageTracker;
use crate::session;
use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame, Terminal,
};
use std::io;
use tokio::sync::mpsc;

struct DisplayMessage {
    role: Role,
    content: String,
}

#[derive(PartialEq)]
enum Role {
    User,
    Assistant,
    Tool,
    Error,
}

enum UiStatus {
    Idle,
    Thinking,
    UsingTool(String),
}

pub struct App {
    model_name: String,
    messages: Vec<DisplayMessage>,
    input: String,
    cursor_pos: usize,
    scroll_offset: u16,
    ui_status: UiStatus,
    current_response: String,
    should_quit: bool,
    usage_tracker: Option<UsageTracker>,
}

impl App {
    pub fn new(model_name: String) -> Self {
        Self {
            model_name,
            messages: vec![DisplayMessage {
                role: Role::Assistant,
                content: "Welcome to TinyClaw! Type a message and press Enter.".into(),
            }],
            input: String::new(),
            cursor_pos: 0,
            scroll_offset: 0,
            ui_status: UiStatus::Idle,
            current_response: String::new(),
            should_quit: false,
            usage_tracker: None,
        }
    }

    pub async fn run(mut self, agent: AgentState) -> Result<()> {
        self.usage_tracker = Some(agent.usage_tracker.clone());
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        let result = self.event_loop(&mut terminal, agent).await;

        disable_raw_mode()?;
        execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
        terminal.show_cursor()?;

        result
    }

    async fn event_loop(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
        agent: AgentState,
    ) -> Result<()> {
        let (event_tx, mut event_rx) = mpsc::channel::<AgentEvent>(64);

        // Channel to get agent back after task completes
        let (agent_return_tx, mut agent_return_rx) =
            mpsc::channel::<AgentState>(1);

        let mut agent_opt: Option<AgentState> = Some(agent);
        let mut agent_running = false;

        loop {
            terminal.draw(|f| self.draw(f))?;

            if self.should_quit {
                break;
            }

            // Check if agent task finished and returned the agent state
            if agent_running {
                match agent_return_rx.try_recv() {
                    Ok(returned_agent) => {
                        agent_opt = Some(returned_agent);
                        agent_running = false;
                    }
                    Err(mpsc::error::TryRecvError::Empty) => {}
                    Err(mpsc::error::TryRecvError::Disconnected) => {
                        agent_running = false;
                    }
                }
            }

            // Drain agent events
            while let Ok(evt) = event_rx.try_recv() {
                self.handle_agent_event(evt);
            }

            // Poll terminal events
            if event::poll(std::time::Duration::from_millis(50))? {
                if let Event::Key(key) = event::read()? {
                    match (key.code, key.modifiers) {
                        (KeyCode::Char('c'), m) if m.contains(KeyModifiers::CONTROL) => {
                            self.should_quit = true;
                        }
                        (KeyCode::Enter, _) if !agent_running => {
                            if !self.input.trim().is_empty() {
                                let user_msg = self.input.clone();
                                self.input.clear();
                                self.cursor_pos = 0;

                                if user_msg.trim() == "/quit" || user_msg.trim() == "/exit" {
                                    self.should_quit = true;
                                    continue;
                                }

                                // Handle slash commands locally
                                if let Some(response) = self.handle_slash_command(
                                    user_msg.trim(),
                                    &mut agent_opt,
                                ) {
                                    self.messages.push(DisplayMessage {
                                        role: Role::Assistant,
                                        content: response,
                                    });
                                    self.scroll_offset = 0;
                                    continue;
                                }

                                self.messages.push(DisplayMessage {
                                    role: Role::User,
                                    content: user_msg.clone(),
                                });
                                self.ui_status = UiStatus::Thinking;
                                self.current_response.clear();
                                self.scroll_offset = 0;

                                // Move agent into spawned task, get it back when done
                                if let Some(mut ag) = agent_opt.take() {
                                    agent_running = true;
                                    let tx = event_tx.clone();
                                    let return_tx = agent_return_tx.clone();
                                    tokio::spawn(async move {
                                        ag.handle_message(&user_msg, &tx).await;
                                        let _ = return_tx.send(ag).await;
                                    });
                                }
                            }
                        }
                        (KeyCode::Char(c), _) if !agent_running => {
                            self.input.insert(self.cursor_pos, c);
                            self.cursor_pos += 1;
                        }
                        (KeyCode::Backspace, _) if !agent_running => {
                            if self.cursor_pos > 0 {
                                self.cursor_pos -= 1;
                                self.input.remove(self.cursor_pos);
                            }
                        }
                        (KeyCode::Left, _) if !agent_running => {
                            self.cursor_pos = self.cursor_pos.saturating_sub(1);
                        }
                        (KeyCode::Right, _) if !agent_running => {
                            if self.cursor_pos < self.input.len() {
                                self.cursor_pos += 1;
                            }
                        }
                        (KeyCode::Home, _) => self.cursor_pos = 0,
                        (KeyCode::End, _) => self.cursor_pos = self.input.len(),
                        (KeyCode::PageUp, _) => {
                            self.scroll_offset = self.scroll_offset.saturating_add(10);
                        }
                        (KeyCode::PageDown, _) => {
                            self.scroll_offset = self.scroll_offset.saturating_sub(10);
                        }
                        _ => {}
                    }
                }
            }
        }

        Ok(())
    }

    fn handle_agent_event(&mut self, evt: AgentEvent) {
        match evt {
            AgentEvent::Token(text) => {
                self.current_response.push_str(&text);
                self.scroll_offset = 0;
            }
            AgentEvent::ToolStart(name) => {
                self.ui_status = UiStatus::UsingTool(name);
            }
            AgentEvent::ToolResult { name, preview } => {
                self.messages.push(DisplayMessage {
                    role: Role::Tool,
                    content: format!("[{name}] {preview}"),
                });
                self.ui_status = UiStatus::Thinking;
            }
            AgentEvent::Done(response) => {
                let content = if self.current_response.is_empty() {
                    response
                } else {
                    self.current_response.clone()
                };
                self.messages.push(DisplayMessage {
                    role: Role::Assistant,
                    content,
                });
                self.current_response.clear();
                self.ui_status = UiStatus::Idle;
            }
            AgentEvent::Error(err) => {
                self.messages.push(DisplayMessage {
                    role: Role::Error,
                    content: err,
                });
                self.current_response.clear();
                self.ui_status = UiStatus::Idle;
            }
        }
    }

    fn draw(&self, frame: &mut Frame) {
        let size = frame.area();
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Min(5),
                Constraint::Length(1),
                Constraint::Length(3),
            ])
            .split(size);

        self.draw_header(frame, chunks[0]);
        self.draw_messages(frame, chunks[1]);
        self.draw_status(frame, chunks[2]);
        self.draw_input(frame, chunks[3]);
    }

    fn draw_header(&self, frame: &mut Frame, area: Rect) {
        let header = Paragraph::new(Line::from(vec![
            Span::styled(
                " TinyClaw ",
                Style::default().fg(Color::Black).bg(Color::Cyan),
            ),
            Span::raw(" "),
            Span::styled(
                self.model_name.clone(),
                Style::default().fg(Color::DarkGray),
            ),
            Span::raw("  Ctrl+C quit  PageUp/Down scroll"),
        ]));
        frame.render_widget(header, area);
    }

    fn draw_messages(&self, frame: &mut Frame, area: Rect) {
        let mut lines: Vec<Line> = Vec::new();

        for msg in &self.messages {
            let (prefix, style) = match msg.role {
                Role::User => (
                    "You",
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                ),
                Role::Assistant => ("AI", Style::default().fg(Color::Cyan)),
                Role::Tool => ("Tool", Style::default().fg(Color::Yellow)),
                Role::Error => (
                    "Error",
                    Style::default()
                        .fg(Color::Red)
                        .add_modifier(Modifier::BOLD),
                ),
            };

            lines.push(Line::from(Span::styled(format!("{prefix}: "), style)));
            let rendered = super::markdown::render_to_spans(&msg.content);
            lines.extend(rendered);
            lines.push(Line::from(""));
        }

        if !self.current_response.is_empty() {
            lines.push(Line::from(Span::styled(
                "AI: ",
                Style::default().fg(Color::Cyan),
            )));
            let rendered = super::markdown::render_to_spans(&self.current_response);
            lines.extend(rendered);
            lines.push(Line::from(Span::styled(
                " ...",
                Style::default().fg(Color::DarkGray),
            )));
        }

        let total_lines = lines.len() as u16;
        let visible = area.height.saturating_sub(2);
        let max_scroll = total_lines.saturating_sub(visible);
        let scroll = if self.scroll_offset == 0 {
            max_scroll
        } else {
            max_scroll.saturating_sub(self.scroll_offset)
        };

        let messages_widget = Paragraph::new(lines)
            .block(Block::default().borders(Borders::LEFT | Borders::RIGHT))
            .wrap(Wrap { trim: false })
            .scroll((scroll, 0));

        frame.render_widget(messages_widget, area);
    }

    fn draw_status(&self, frame: &mut Frame, area: Rect) {
        let (text, color) = match &self.ui_status {
            UiStatus::Idle => ("Ready".to_string(), Color::Green),
            UiStatus::Thinking => ("Thinking...".to_string(), Color::Yellow),
            UiStatus::UsingTool(name) => (format!("Running {name}..."), Color::Magenta),
        };

        let usage_text = if let Some(tracker) = &self.usage_tracker {
            let total = tracker.snapshot().total_tokens;
            let cost = tracker.estimated_cost_usd();
            let reqs = tracker.requests();
            if total > 0 {
                format!("  [{total} tokens, {reqs} reqs, ~${cost:.4}]")
            } else {
                String::new()
            }
        } else {
            String::new()
        };

        let status = Paragraph::new(Line::from(vec![
            Span::styled(" Status: ", Style::default().fg(Color::DarkGray)),
            Span::styled(text, Style::default().fg(color)),
            Span::styled(usage_text, Style::default().fg(Color::DarkGray)),
        ]));
        frame.render_widget(status, area);
    }

    /// Handle TUI slash commands. Returns Some(response) if handled, None otherwise.
    fn handle_slash_command(
        &mut self,
        cmd: &str,
        agent_opt: &mut Option<AgentState>,
    ) -> Option<String> {
        let parts: Vec<&str> = cmd.splitn(2, ' ').collect();
        let command = parts[0];
        let _arg = parts.get(1).copied().unwrap_or("");

        match command {
            "/help" => Some(
                "Available commands:\n\
                 /help     - Show this help\n\
                 /cost     - Show token usage and estimated cost\n\
                 /clear    - Clear message history (keeps system prompt)\n\
                 /model    - Show current model\n\
                 /sessions - List saved sessions\n\
                 /session  - Show current session ID\n\
                 /export   - Export conversation to file\n\
                 /quit     - Exit TinyClaw"
                    .to_string(),
            ),
            "/cost" => {
                if let Some(tracker) = &self.usage_tracker {
                    let snap = tracker.snapshot();
                    let cost = tracker.estimated_cost_usd();
                    let reqs = tracker.requests();
                    Some(format!(
                        "Token Usage:\n\
                         Prompt tokens:     {}\n\
                         Completion tokens: {}\n\
                         Total tokens:      {}\n\
                         Requests:          {reqs}\n\
                         Estimated cost:    ${cost:.4}",
                        snap.prompt_tokens, snap.completion_tokens, snap.total_tokens,
                    ))
                } else {
                    Some("Usage tracking not available.".to_string())
                }
            }
            "/clear" => {
                if let Some(ag) = agent_opt.as_mut() {
                    // Keep only the system prompt
                    ag.history.retain(|m| m.role == "system");
                }
                self.messages.clear();
                self.current_response.clear();
                self.scroll_offset = 0;
                Some("Conversation cleared.".to_string())
            }
            "/model" => {
                let model = agent_opt
                    .as_ref()
                    .map(|a| a.model.as_str())
                    .unwrap_or("unknown");
                Some(format!("Current model: {model}"))
            }
            "/session" => {
                let id = agent_opt
                    .as_ref()
                    .map(|a| a.session_id.as_str())
                    .unwrap_or("unknown");
                Some(format!("Session ID: {id}"))
            }
            "/sessions" => {
                if let Some(ag) = agent_opt.as_ref() {
                    match session::list(&ag.workspace_dir) {
                        Ok(sessions) if sessions.is_empty() => {
                            Some("No saved sessions.".to_string())
                        }
                        Ok(sessions) => {
                            let mut out = String::from("Saved sessions:\n");
                            for s in sessions.iter().take(10) {
                                out.push_str(&format!(
                                    "  {} ({} msgs) - {}\n",
                                    s.id, s.message_count, s.preview
                                ));
                            }
                            Some(out)
                        }
                        Err(e) => Some(format!("Error listing sessions: {e}")),
                    }
                } else {
                    Some("Agent not available.".to_string())
                }
            }
            "/export" => {
                if let Some(ag) = agent_opt.as_ref() {
                    let path = ag.workspace_dir.join("exports");
                    let _ = std::fs::create_dir_all(&path);
                    let file = path.join(format!("{}.md", ag.session_id));
                    let mut content = String::new();
                    for msg in &self.messages {
                        let label = match msg.role {
                            Role::User => "**You**",
                            Role::Assistant => "**AI**",
                            Role::Tool => "**Tool**",
                            Role::Error => "**Error**",
                        };
                        content.push_str(&format!("{label}: {}\n\n", msg.content));
                    }
                    match std::fs::write(&file, &content) {
                        Ok(()) => Some(format!("Exported to {}", file.display())),
                        Err(e) => Some(format!("Export failed: {e}")),
                    }
                } else {
                    Some("Agent not available.".to_string())
                }
            }
            _ if cmd.starts_with('/') => {
                Some(format!("Unknown command: {command}. Type /help for available commands."))
            }
            _ => None,
        }
    }

    fn draw_input(&self, frame: &mut Frame, area: Rect) {
        let input_widget = Paragraph::new(self.input.as_str())
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Message (/quit to exit) ")
                    .border_style(Style::default().fg(match self.ui_status {
                        UiStatus::Idle => Color::Cyan,
                        _ => Color::DarkGray,
                    })),
            )
            .wrap(Wrap { trim: false });
        frame.render_widget(input_widget, area);

        let cursor_x = area.x + 1 + self.cursor_pos as u16;
        let cursor_y = area.y + 1;
        frame.set_cursor_position((cursor_x, cursor_y));
    }
}
