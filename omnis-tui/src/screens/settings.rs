/// Settings screen — API URL, theme, sessions, connection test, log viewer.
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
    Frame,
};

use crate::components::input_box::InputBox;
use crate::screens::AppAction;
use crate::theme::Theme;
use crate::types::{AppSettings, WireSession};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsItem {
    ApiUrl,
    ThemeColor,
    RunInBackground,
    TestConnection,
    Sessions,
    RevokeOther,
    OpenLogs,
    Logout,
}

pub const ITEMS: &[SettingsItem] = &[
    SettingsItem::ApiUrl,
    SettingsItem::ThemeColor,
    SettingsItem::RunInBackground,
    SettingsItem::TestConnection,
    SettingsItem::Sessions,
    SettingsItem::RevokeOther,
    SettingsItem::OpenLogs,
    SettingsItem::Logout,
];

pub struct SettingsScreen {
    list_state: ListState,
    editing: bool,
    edit_input: InputBox,
    pub status: String,
    pub sessions: Vec<WireSession>,
    pub logs: Vec<String>,
    show_logs: bool,
    log_scroll: usize,
}

impl SettingsScreen {
    pub fn new() -> Self {
        let mut list_state = ListState::default();
        list_state.select(Some(0));
        Self {
            list_state,
            editing: false,
            edit_input: InputBox::new(""),
            status: String::new(),
            sessions: Vec::new(),
            logs: Vec::new(),
            show_logs: false,
            log_scroll: 0,
        }
    }

    fn selected_item(&self) -> SettingsItem {
        ITEMS[self.list_state.selected().unwrap_or(0)]
    }

    /// Returns the action to take plus whether settings values changed.
    pub fn handle_key(
        &mut self,
        key: KeyEvent,
        settings: &AppSettings,
    ) -> (Option<AppAction>, Option<AppSettings>) {
        if self.show_logs {
            match key.code {
                KeyCode::Esc | KeyCode::Char('l') => {
                    self.show_logs = false;
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    self.log_scroll = self.log_scroll.saturating_sub(1);
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    self.log_scroll = (self.log_scroll + 1).min(self.logs.len().saturating_sub(1));
                }
                _ => {}
            }
            return (None, None);
        }

        if self.editing {
            match key.code {
                KeyCode::Esc => {
                    self.editing = false;
                }
                KeyCode::Enter => {
                    let value = self.edit_input.value.trim().to_string();
                    self.editing = false;
                    let mut new_settings = settings.clone();
                    let action = match self.selected_item() {
                        SettingsItem::ApiUrl => {
                            new_settings.api_base_url = value.clone();
                            Some(AppAction::ApiUrlChanged(value))
                        }
                        SettingsItem::ThemeColor => {
                            new_settings.theme_color = value.clone();
                            Some(AppAction::ThemeChanged(value))
                        }
                        _ => None,
                    };
                    return (action, Some(new_settings));
                }
                _ => {
                    self.edit_input.handle_key(key);
                }
            }
            return (None, None);
        }

        match key.code {
            KeyCode::Esc => (Some(AppAction::Back), None),
            KeyCode::Up | KeyCode::Char('k') => {
                let len = ITEMS.len();
                let i = self.list_state.selected().unwrap_or(0);
                self.list_state.select(Some(if i == 0 { len - 1 } else { i - 1 }));
                (None, None)
            }
            KeyCode::Down | KeyCode::Char('j') => {
                let len = ITEMS.len();
                let i = self.list_state.selected().unwrap_or(0);
                self.list_state.select(Some((i + 1) % len));
                (None, None)
            }
            KeyCode::Enter => {
                let action = match self.selected_item() {
                    SettingsItem::ApiUrl => {
                        self.edit_input.set_value(&settings.api_base_url);
                        self.editing = true;
                        None
                    }
                    SettingsItem::ThemeColor => {
                        self.edit_input.set_value(&settings.theme_color);
                        self.editing = true;
                        None
                    }
                    SettingsItem::RunInBackground => {
                        let mut new_settings = settings.clone();
                        new_settings.run_in_background = !settings.run_in_background;
                        return (None, Some(new_settings));
                    }
                    SettingsItem::TestConnection => {
                        self.status = "Testing…".to_string();
                        None // App will do the async test
                    }
                    SettingsItem::Sessions => {
                        self.status = "Fetching sessions…".to_string();
                        None // App will fetch
                    }
                    SettingsItem::RevokeOther => {
                        self.status = "Revoking other sessions…".to_string();
                        None
                    }
                    SettingsItem::OpenLogs => {
                        self.show_logs = true;
                        self.log_scroll = self.logs.len().saturating_sub(1);
                        None
                    }
                    SettingsItem::Logout => Some(AppAction::Logout),
                };
                (action, None)
            }
            _ => (None, None),
        }
    }

    /// Which item was just activated (for App to handle async ops).
    pub fn last_activated(&self) -> SettingsItem {
        self.selected_item()
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect, settings: &AppSettings, theme: &Theme) {
        let outer = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.border))
            .title(Span::styled(" Settings ", Style::default().fg(theme.accent)));
        let inner = outer.inner(area);
        frame.render_widget(outer, area);

        if self.show_logs {
            self.render_logs(frame, inner, theme);
            return;
        }

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(0),   // settings list
                Constraint::Length(if self.editing { 3 } else { 0 }), // edit input
                Constraint::Length(if !self.sessions.is_empty() { (self.sessions.len() as u16 + 2).min(8) } else { 0 }), // sessions
                Constraint::Length(1), // status
                Constraint::Length(1), // hint
            ])
            .split(inner);

        let items: Vec<ListItem> = ITEMS
            .iter()
            .map(|item| {
                let (label, value) = match item {
                    SettingsItem::ApiUrl => ("API URL", settings.api_base_url.as_str()),
                    SettingsItem::ThemeColor => ("Theme Color", settings.theme_color.as_str()),
                    SettingsItem::RunInBackground => (
                        "Run in Background",
                        if settings.run_in_background { "☑ On" } else { "☐ Off" },
                    ),
                    SettingsItem::TestConnection => ("Test Connection", "→ Enter"),
                    SettingsItem::Sessions => ("View Sessions", "→ Enter"),
                    SettingsItem::RevokeOther => ("Revoke Other Sessions", "→ Enter"),
                    SettingsItem::OpenLogs => ("View Logs", "→ Enter"),
                    SettingsItem::Logout => ("Logout", "→ Enter"),
                };
                ListItem::new(Line::from(vec![
                    Span::styled(
                        format!("{:<24}", label),
                        Style::default().fg(theme.text),
                    ),
                    Span::styled(value, Style::default().fg(theme.muted)),
                ]))
            })
            .collect();

        let list = List::new(items)
            .block(Block::default().borders(Borders::NONE))
            .highlight_style(Style::default().bg(theme.border).add_modifier(Modifier::BOLD))
            .highlight_symbol("▶ ");
        frame.render_stateful_widget(list, chunks[0], &mut self.list_state);

        if self.editing {
            let field_name = match self.selected_item() {
                SettingsItem::ApiUrl => "API URL",
                SettingsItem::ThemeColor => "Theme Color",
                _ => "Value",
            };
            self.edit_input.render(frame, chunks[1], theme, true, field_name);
        }

        // Sessions list
        if !self.sessions.is_empty() && chunks[2].height > 0 {
            let session_items: Vec<ListItem> = self
                .sessions
                .iter()
                .map(|s| {
                    let marker = if s.is_current { "● " } else { "○ " };
                    let label = format!(
                        "{}{} — {}",
                        marker,
                        s.device_id.chars().take(12).collect::<String>(),
                        s.last_accessed.format("%Y-%m-%d %H:%M")
                    );
                    ListItem::new(Span::styled(
                        label,
                        if s.is_current {
                            Style::default().fg(theme.success)
                        } else {
                            Style::default().fg(theme.muted)
                        },
                    ))
                })
                .collect();
            let sessions_list = List::new(session_items).block(
                Block::default()
                    .borders(Borders::TOP)
                    .border_style(Style::default().fg(theme.border))
                    .title(Span::styled(" Active Sessions ", Style::default().fg(theme.muted))),
            );
            frame.render_widget(sessions_list, chunks[2]);
        }

        // Status
        frame.render_widget(
            Paragraph::new(self.status.as_str()).style(Style::default().fg(theme.muted)),
            chunks[3],
        );

        // Hint
        frame.render_widget(
            Paragraph::new("↑↓ navigate  Enter edit/activate  Esc back")
                .style(Style::default().fg(theme.muted)),
            chunks[4],
        );
    }

    fn render_logs(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let start = self.log_scroll.min(self.logs.len().saturating_sub(1));
        let lines: Vec<Line> = self
            .logs
            .iter()
            .skip(start)
            .take(area.height as usize)
            .map(|l| Line::from(Span::styled(l.as_str(), Style::default().fg(theme.muted))))
            .collect();
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.border))
            .title(Span::styled(" Logs  Esc back ", Style::default().fg(theme.muted)));
        frame.render_widget(Paragraph::new(lines).block(block), area);
    }
}
