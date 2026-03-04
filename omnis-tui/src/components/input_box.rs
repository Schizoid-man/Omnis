/// Single-line text input with cursor support.
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use crate::theme::Theme;

pub struct InputBox {
    pub value: String,
    cursor: usize, // byte offset
    pub placeholder: String,
    /// If true, render value as bullet characters instead of plain text.
    pub secret: bool,
}

impl InputBox {
    pub fn new(placeholder: impl Into<String>) -> Self {
        Self {
            value: String::new(),
            cursor: 0,
            placeholder: placeholder.into(),
            secret: false,
        }
    }

    pub fn set_value(&mut self, v: impl Into<String>) {
        self.value = v.into();
        self.cursor = self.value.len();
    }

    pub fn clear(&mut self) {
        self.value.clear();
        self.cursor = 0;
    }

    /// Handle a key event. Returns true if the event was consumed.
    pub fn handle_key(&mut self, key: KeyEvent) -> bool {
        match key.code {
            // Ctrl+U — clear line (must come before the generic Char match)
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.value.drain(..self.cursor);
                self.cursor = 0;
                true
            }
            KeyCode::Char(c) => {
                self.value.insert(self.cursor, c);
                self.cursor += c.len_utf8();
                true
            }
            KeyCode::Backspace => {
                if self.cursor > 0 {
                    // find previous char boundary
                    let prev = self.prev_char_boundary();
                    self.value.drain(prev..self.cursor);
                    self.cursor = prev;
                }
                true
            }
            KeyCode::Delete => {
                if self.cursor < self.value.len() {
                    let next = self.next_char_boundary();
                    self.value.drain(self.cursor..next);
                }
                true
            }
            KeyCode::Left => {
                if self.cursor > 0 {
                    self.cursor = self.prev_char_boundary();
                }
                true
            }
            KeyCode::Right => {
                if self.cursor < self.value.len() {
                    self.cursor = self.next_char_boundary();
                }
                true
            }
            KeyCode::Home => {
                self.cursor = 0;
                true
            }
            KeyCode::End => {
                self.cursor = self.value.len();
                true
            }
            _ => false,
        }
    }

    fn prev_char_boundary(&self) -> usize {
        let mut i = self.cursor - 1;
        while i > 0 && !self.value.is_char_boundary(i) {
            i -= 1;
        }
        i
    }

    fn next_char_boundary(&self) -> usize {
        let mut i = self.cursor + 1;
        while i < self.value.len() && !self.value.is_char_boundary(i) {
            i += 1;
        }
        i
    }

    pub fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme, focused: bool, title: &str) {
        let border_style = if focused {
            Style::default().fg(theme.accent)
        } else {
            Style::default().fg(theme.border)
        };

        // Build display — password fields show bullets instead of plain text
        let masked: String;
        let display: Line = if self.value.is_empty() {
            Line::from(Span::styled(
                self.placeholder.as_str(),
                Style::default().fg(theme.muted),
            ))
        } else if self.secret {
            masked = "\u{2022}".repeat(self.value.chars().count());
            Line::from(masked.as_str())
        } else {
            Line::from(self.value.as_str())
        };

        let paragraph = Paragraph::new(display).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(border_style)
                .title(Span::styled(title, Style::default().fg(theme.muted))),
        );
        frame.render_widget(paragraph, area);

        // Render cursor
        if focused {
            let x = area.x + 1 + self.value[..self.cursor].chars().count() as u16;
            let y = area.y + 1;
            if x < area.x + area.width - 1 && y < area.y + area.height - 1 {
                frame.set_cursor_position((x, y));
            }
        }
    }
}
