use crate::agent::loop_rs::Agent;
use crate::models::{CancelToken, Session};
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::ExecutableCommand;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};
use ratatui::Frame;
use std::io::stdout;

struct ChatMessage {
    role: String,
    content: String,
}

pub struct TuiChat {
    messages: Vec<ChatMessage>,
    input: String,
    cursor_pos: usize,
    scroll_offset: usize,
    is_thinking: bool,
    stream_buffer: String,
    cancel: CancelToken,
}

fn wrap_text(text: &str, max_width: usize) -> Vec<String> {
    let max = max_width.max(1);
    let mut lines = Vec::new();
    for paragraph in text.split('\n') {
        if paragraph.len() <= max {
            lines.push(paragraph.to_string());
        } else {
            let mut start = 0;
            while start < paragraph.len() {
                let end = (start + max).min(paragraph.len());
                let break_at = if end < paragraph.len() {
                    paragraph[start..end]
                        .rfind(|c: char| c.is_whitespace())
                        .map(|pos| start + pos + 1)
                        .unwrap_or(end)
                } else {
                    end
                };
                lines.push(paragraph[start..break_at].to_string());
                start = break_at;
            }
        }
    }
    lines
}

impl TuiChat {
    pub fn new(cancel: CancelToken) -> Self {
        Self {
            messages: Vec::new(),
            input: String::new(),
            cursor_pos: 0,
            scroll_offset: 0,
            is_thinking: false,
            stream_buffer: String::new(),
            cancel,
        }
    }

    fn add_message(&mut self, role: &str, content: &str) {
        self.messages.push(ChatMessage {
            role: role.to_string(),
            content: content.to_string(),
        });
        self.scroll_offset = 0;
    }

    pub async fn run(agent: &Agent) -> anyhow::Result<()> {
        enable_raw_mode()?;
        stdout().execute(EnterAlternateScreen)?;
        let mut terminal = ratatui::Terminal::new(ratatui::backend::CrosstermBackend::new(stdout()))?;
        terminal.clear()?;

        let cancel = CancelToken::new();
        let c = cancel.clone();
        tokio::spawn(async move {
            let _ = tokio::signal::ctrl_c().await;
            c.cancel();
        });

        let mut chat = TuiChat::new(cancel);
        let sessions_pool = crate::session::open_sessions(
            &std::path::PathBuf::from("volt_sessions.db"),
        )
        .await
        .ok();

        {
            let state = agent.state.lock().await;
            for msg in &state.messages {
                chat.messages.push(ChatMessage {
                    role: msg.role.clone(),
                    content: msg.content.clone(),
                });
            }
        }

        loop {
            terminal.draw(|f| chat.render(f))?;

            if chat.cancel.is_cancelled() {
                if chat.is_thinking {
                    chat.stream_buffer.push_str("\n[interrupted]");
                    let buf = chat.stream_buffer.clone();
                    chat.add_message("assistant", &buf);
                    chat.stream_buffer.clear();
                    chat.is_thinking = false;
                } else {
                    break;
                }
            }

            if chat.is_thinking {
                let input = chat.input.clone();
                let result = agent.run(&input).await;

                if chat.cancel.is_cancelled() {
                    chat.stream_buffer.clear();
                    chat.is_thinking = false;
                    continue;
                }

                match result {
                    Ok(output) => {
                        chat.add_message("assistant", &output);
                        if let Some(ref sp) = sessions_pool {
                            let s = agent.state.lock().await;
                            let _ = crate::session::create_session(
                                sp,
                                &Session {
                                    id: s.session_id,
                                    agent_name: s.name.clone(),
                                    title: input.chars().take(60).collect(),
                                    message_count: s.messages.len() as u32,
                                    created_at: s.created_at,
                                    updated_at: s.updated_at,
                                },
                            )
                            .await;
                            for msg in s.messages.iter().rev().take(2) {
                                let _ = crate::session::save_message(sp, s.session_id, msg).await;
                            }
                        }
                    }
                    Err(e) => {
                        chat.add_message("system", &format!("error: {}", e));
                    }
                }
                chat.is_thinking = false;
                continue;
            }

            if !matches!(event::poll(std::time::Duration::from_millis(100))?, true) {
                continue;
            }

            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    match key.code {
                        KeyCode::Enter => {
                            let input = chat.input.trim().to_string();
                            if input.is_empty() {
                                continue;
                            }
                            if input == "/quit" {
                                break;
                            }
                            chat.add_message("user", &input);
                            chat.input.clear();
                            chat.cursor_pos = 0;
                            chat.is_thinking = true;
                            chat.stream_buffer.clear();
                        }
                        KeyCode::Backspace => {
                            if chat.cursor_pos > 0 {
                                chat.cursor_pos -= 1;
                                chat.input.remove(chat.cursor_pos);
                            }
                        }
                        KeyCode::Delete => {
                            if chat.cursor_pos < chat.input.len() {
                                chat.input.remove(chat.cursor_pos);
                            }
                        }
                        KeyCode::Char(c) => {
                            chat.input.insert(chat.cursor_pos, c);
                            chat.cursor_pos += 1;
                        }
                        KeyCode::Left => {
                            chat.cursor_pos = chat.cursor_pos.saturating_sub(1);
                        }
                        KeyCode::Right => {
                            chat.cursor_pos = chat.cursor_pos.saturating_add(1).min(chat.input.len());
                        }
                        KeyCode::Home => {
                            chat.cursor_pos = 0;
                        }
                        KeyCode::End => {
                            chat.cursor_pos = chat.input.len();
                        }
                        KeyCode::Up => {
                            let max = chat.messages.len().saturating_sub(1);
                            chat.scroll_offset = (chat.scroll_offset + 1).min(max);
                        }
                        KeyCode::Down => {
                            chat.scroll_offset = chat.scroll_offset.saturating_sub(1);
                        }
                        KeyCode::PageUp => {
                            let max = chat.messages.len().saturating_sub(1);
                            chat.scroll_offset = (chat.scroll_offset + 10).min(max);
                        }
                        KeyCode::PageDown => {
                            chat.scroll_offset = chat.scroll_offset.saturating_sub(10);
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }

        disable_raw_mode()?;
        stdout().execute(LeaveAlternateScreen)?;
        Ok(())
    }

    fn render(&self, f: &mut Frame) {
        let area = f.area();
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(3), Constraint::Length(3)])
            .split(area);

        self.render_messages(f, chunks[0]);
        self.render_input(f, chunks[1]);
    }

    fn render_messages(&self, f: &mut Frame, area: Rect) {
        let max_width = area.width.saturating_sub(4) as usize;

        let items: Vec<ListItem> = self
            .messages
            .iter()
            .map(|m| {
                let style = match m.role.as_str() {
                    "user" => Style::default().fg(Color::Cyan),
                    "assistant" => Style::default().fg(Color::Green),
                    "system" => Style::default().fg(Color::Yellow),
                    "tool" => Style::default().fg(Color::Magenta),
                    _ => Style::default(),
                };
                let role_tag = Span::styled(
                    format!("[{}] ", m.role.to_uppercase()),
                    style.add_modifier(Modifier::BOLD),
                );
                let wrapped = wrap_text(&m.content, max_width);
                let mut lines: Vec<Line> = Vec::with_capacity(wrapped.len() + 1);
                lines.push(Line::from(role_tag));
                for line in wrapped {
                    lines.push(Line::from(Span::raw(line)));
                }
                lines.push(Line::from(Span::raw(String::new())));
                ListItem::new(lines)
            })
            .collect();

        let title = format!(" Volt Agent Chat ({} msgs) ", self.messages.len());
        let messages = List::new(items)
            .block(Block::default().borders(Borders::ALL).title(title));

        let visible = area.height.saturating_sub(2) as usize;
        let total = self.messages.len();
        let offset = if self.scroll_offset == 0 {
            total.saturating_sub(visible)
        } else {
            self.scroll_offset
        };

        let mut state = ratatui::widgets::ListState::default();
        if offset > 0 {
            state = state.with_offset(offset);
        }
        f.render_stateful_widget(messages, area, &mut state);
    }

    fn render_input(&self, f: &mut Frame, area: Rect) {
        let prefix = if self.is_thinking {
            " thinking... "
        } else {
            " input > "
        };
        let input = Paragraph::new(self.input.as_str())
            .block(Block::default().borders(Borders::ALL).title(prefix))
            .style(if self.is_thinking {
                Style::default().fg(Color::DarkGray)
            } else {
                Style::default()
            });
        f.render_widget(input, area);

        if !self.is_thinking {
            let x = area.x + 1 + self.cursor_pos as u16;
            let y = area.y + 1;
            f.set_cursor_position(ratatui::prelude::Position::new(x.min(area.right().saturating_sub(1)), y));
        }
    }
}
