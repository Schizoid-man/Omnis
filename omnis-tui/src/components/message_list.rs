/// Scrollable message list widget with reply threading.
use chrono::{Local, Utc};
use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState},
    Frame,
};

use crate::theme::Theme;
use crate::types::{DownloadState, LocalMessage};

pub struct MessageList {
    pub state: ListState,
    /// Scroll offset from the bottom (0 = newest visible at bottom)
    pub scroll_offset: usize,
}

impl MessageList {
    pub fn new() -> Self {
        Self {
            state: ListState::default(),
            scroll_offset: 0,
        }
    }

    pub fn scroll_up(&mut self) {
        self.scroll_offset += 1;
    }

    pub fn scroll_down(&mut self) {
        self.scroll_offset = self.scroll_offset.saturating_sub(1);
    }

    pub fn scroll_to_bottom(&mut self) {
        self.scroll_offset = 0;
    }

    pub fn selected_index(&self) -> Option<usize> {
        self.state.selected()
    }

    pub fn move_selection_up(&mut self, len: usize) {
        if len == 0 {
            return;
        }
        let i = self.state.selected().unwrap_or(len.saturating_sub(1));
        let next = if i == 0 { 0 } else { i - 1 };
        self.state.select(Some(next));
    }

    pub fn move_selection_down(&mut self, len: usize) {
        if len == 0 {
            return;
        }
        let i = self.state.selected().unwrap_or(0);
        let next = (i + 1).min(len - 1);
        self.state.select(Some(next));
    }

    pub fn render(
        &mut self,
        frame: &mut Frame,
        area: Rect,
        messages: &[LocalMessage],
        my_user_id: i64,
        with_user: &str,
        theme: &Theme,
    ) {
        let items: Vec<ListItem> = messages
            .iter()
            .map(|msg| {
                let is_mine = msg.sender_id == my_user_id;
                let name_style = if is_mine {
                    Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(theme.success).add_modifier(Modifier::BOLD)
                };

                let sender_str: String = if is_mine { "You".to_string() } else { with_user.to_string() };
                let time = msg
                    .created_at
                    .with_timezone(&Local)
                    .format("%H:%M")
                    .to_string();

                // For media messages show the caption; for text messages show the plaintext.
                // The raw JSON envelope is NEVER shown directly to the user.
                let body: String = match &msg.media_info {
                    Some(info) => info.caption.clone(),
                    None => msg.plaintext.as_deref().unwrap_or("[encrypted]").to_string(),
                };

                let mut lines = Vec::new();

                // If this message is a reply, show a quote line
                if msg.reply_id.is_some() {
                    lines.push(Line::from(Span::styled(
                        "  ╭ replied to a message",
                        Style::default().fg(theme.muted),
                    )));
                }

                // Header: sender + time
                lines.push(Line::from(vec![
                    Span::styled(sender_str.clone(), name_style),
                    Span::styled(format!("  {}", time), Style::default().fg(theme.muted)),
                ]));

                // Only show body text if non-empty (skip for media-only messages with no caption)
                if !body.is_empty() {
                    let max_w = (area.width.saturating_sub(4)) as usize;
                    for chunk in wrap_text(&body, max_w) {
                        lines.push(Line::from(Span::raw(chunk)));
                    }
                }

                // Media attachment info
                if let Some(ref info) = msg.media_info {
                    let attach_line = format!("[ATTACHMENT: {} | {}]", info.file_type, info.filename);
                    lines.push(Line::from(Span::styled(
                        attach_line,
                        Style::default().fg(theme.accent),
                    )));
                    let dl_hint = match &msg.download_state {
                        DownloadState::None => " [Ctrl+D to download]".to_string(),
                        DownloadState::Pending => " Downloading…".to_string(),
                        DownloadState::Downloaded(path) => format!(" Saved: {}", path.display()),
                        DownloadState::Failed(err) => format!(" Download failed: {}", err),
                    };
                    lines.push(Line::from(Span::styled(
                        dl_hint,
                        Style::default().fg(theme.muted),
                    )));
                    // Pixel preview for image attachments (shown after download)
                    if let Some(ref preview) = msg.pixel_preview {
                        for row in preview.iter() {
                            let spans: Vec<Span> = row
                                .iter()
                                .map(|(ch, fg, bg)| {
                                    Span::styled(
                                        ch.to_string(),
                                        Style::default()
                                            .fg(Color::Rgb(fg.0, fg.1, fg.2))
                                            .bg(Color::Rgb(bg.0, bg.1, bg.2)),
                                    )
                                })
                                .collect();
                            lines.push(Line::from(spans));
                        }
                    }                }

                // Spacing line
                lines.push(Line::from(""));

                // Ephemeral timer hint
                if let Some(expires_at) = msg.expires_at {
                    let secs = (expires_at - Utc::now()).num_seconds();
                    if secs > 0 {
                        let label = if secs < 60 {
                            format!("\u{23f3} {}s", secs)
                        } else if secs < 3600 {
                            format!("\u{23f3} {}m {}s", secs / 60, secs % 60)
                        } else {
                            format!("\u{23f3} {}h", secs / 3600)
                        };
                        lines.push(Line::from(Span::styled(
                            label,
                            Style::default().fg(Color::Red),
                        )));
                    }
                }

                ListItem::new(lines)
            })
            .collect();

        let total = items.len();
        let list = List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme.border)),
            )
            .highlight_style(Style::default().bg(theme.border));

        frame.render_stateful_widget(list, area, &mut self.state);
    }
}

fn wrap_text(text: &str, max_width: usize) -> Vec<String> {
    if max_width == 0 {
        return vec![text.to_string()];
    }
    let mut result = Vec::new();
    for line in text.lines() {
        let chars: Vec<char> = line.chars().collect();
        if chars.is_empty() {
            result.push(String::new());
            continue;
        }
        let mut start = 0;
        while start < chars.len() {
            let end = (start + max_width).min(chars.len());
            result.push(chars[start..end].iter().collect());
            start = end;
        }
    }
    result
}
