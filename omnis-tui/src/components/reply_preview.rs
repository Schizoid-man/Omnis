/// Reply preview bar shown above the input box when replying to a message.
use ratatui::{
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use crate::theme::Theme;
use crate::types::LocalMessage;

pub fn render(frame: &mut Frame, area: Rect, reply_to: &LocalMessage, theme: &Theme) {
    let preview: String = match &reply_to.media_info {
        Some(info) => {
            if !info.caption.is_empty() {
                info.caption.chars().take(60).collect()
            } else {
                format!("[{}: {}]", info.file_type, info.filename)
            }
        }
        None => reply_to
            .plaintext
            .as_deref()
            .unwrap_or("[encrypted]")
            .chars()
            .take(60)
            .collect(),
    };

    let line = Line::from(vec![
        Span::styled("↩ Replying: ", Style::default().fg(theme.accent)),
        Span::styled(preview, Style::default().fg(theme.muted)),
        Span::styled("  [Esc to cancel]", Style::default().fg(theme.muted)),
    ]);

    let paragraph = Paragraph::new(line).block(
        Block::default()
            .borders(Borders::LEFT)
            .border_style(Style::default().fg(theme.accent)),
    );
    frame.render_widget(paragraph, area);
}
