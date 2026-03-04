/// Profile overlay — shows username, user ID, and device ID.
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};

use crate::screens::AppAction;
use crate::theme::Theme;
use crate::types::AuthState;

pub struct ProfileScreen;

impl ProfileScreen {
    pub fn handle_key(&self, key: KeyEvent) -> Option<AppAction> {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('p') => Some(AppAction::Back),
            _ => None,
        }
    }

    pub fn render(&self, frame: &mut Frame, area: Rect, auth: &AuthState, theme: &Theme) {
        // Centred popup
        let popup_w = 44u16.min(area.width);
        let popup_h = 10u16.min(area.height);
        let x = area.x + area.width.saturating_sub(popup_w) / 2;
        let y = area.y + area.height.saturating_sub(popup_h) / 2;
        let popup_area = Rect::new(x, y, popup_w, popup_h);

        frame.render_widget(Clear, popup_area);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.accent))
            .title(Span::styled(
                " Profile ",
                Style::default().fg(theme.accent).add_modifier(Modifier::BOLD),
            ));
        let inner = block.inner(popup_area);
        frame.render_widget(block, popup_area);

        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints(vec![Constraint::Length(1); 6])
            .split(inner);

        let field = |label: &str, value: &str| -> Line<'static> {
            Line::from(vec![
                Span::styled(format!("{:<16}", label), Style::default().fg(theme.muted)),
                Span::styled(value.to_string(), Style::default().fg(theme.text)),
            ])
        };

        frame.render_widget(
            Paragraph::new(field("Username:", &auth.username)),
            rows[0],
        );
        frame.render_widget(
            Paragraph::new(field("User ID:", &auth.user_id.to_string())),
            rows[1],
        );
        frame.render_widget(
            Paragraph::new(field("Device ID:", &auth.device_id.chars().take(20).collect::<String>())),
            rows[2],
        );
        frame.render_widget(
            Paragraph::new(field(
                "Public Key:",
                &auth
                    .identity_public_key
                    .as_deref()
                    .unwrap_or("–")
                    .chars()
                    .take(20)
                    .collect::<String>(),
            )),
            rows[3],
        );
        frame.render_widget(ratatui::widgets::Paragraph::new(""), rows[4]);
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "Esc to close",
                Style::default().fg(theme.muted),
            ))),
            rows[5],
        );
    }
}
