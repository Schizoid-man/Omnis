/// Scrollable chat list widget.
use chrono::Local;
use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState},
    Frame,
};

use crate::theme::Theme;
use crate::types::LocalChat;

pub struct ChatList {
    pub state: ListState,
}

impl ChatList {
    pub fn new() -> Self {
        Self {
            state: ListState::default(),
        }
    }

    pub fn select(&mut self, idx: usize) {
        self.state.select(Some(idx));
    }

    pub fn selected(&self) -> Option<usize> {
        self.state.selected()
    }

    pub fn move_up(&mut self, len: usize) {
        if len == 0 {
            return;
        }
        let i = self.state.selected().unwrap_or(0);
        self.state.select(Some(if i == 0 { len - 1 } else { i - 1 }));
    }

    pub fn move_down(&mut self, len: usize) {
        if len == 0 {
            return;
        }
        let i = self.state.selected().unwrap_or(0);
        self.state.select(Some((i + 1) % len));
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect, chats: &[LocalChat], theme: &Theme) {
        let items: Vec<ListItem> = chats
            .iter()
            .map(|chat| {
                let unread = chat.unread_count > 0;
                let name_style = if unread {
                    Style::default().fg(theme.text).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(theme.text)
                };
                let preview = chat
                    .last_message
                    .as_deref()
                    .unwrap_or("")
                    .chars()
                    .take(40)
                    .collect::<String>();
                let time_str = chat
                    .last_message_time
                    .map(|t| {
                        let local = t.with_timezone(&Local);
                        let today = Local::now().date_naive();
                        if local.date_naive() == today {
                            local.format("%H:%M").to_string()
                        } else {
                            local.format("%m/%d").to_string()
                        }
                    })
                    .unwrap_or_default();

                let badge = if unread {
                    format!(" [{}]", chat.unread_count)
                } else {
                    String::new()
                };

                let top_line = Line::from(vec![
                    Span::styled(chat.with_user.as_str(), name_style),
                    Span::styled(badge, Style::default().fg(theme.unread)),
                    Span::styled(
                        format!("{:>width$}", time_str, width = area.width.saturating_sub(
                            chat.with_user.len() as u16 + 4
                        ) as usize),
                        Style::default().fg(theme.muted),
                    ),
                ]);
                let bottom_line = Line::from(Span::styled(preview, Style::default().fg(theme.muted)));
                ListItem::new(vec![top_line, bottom_line])
            })
            .collect();

        let list = List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme.border))
                    .title(Span::styled(" Chats ", Style::default().fg(theme.accent))),
            )
            .highlight_style(
                Style::default()
                    .bg(theme.border)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("▶ ");

        frame.render_stateful_widget(list, area, &mut self.state);
    }
}
