/// Chat screen — real-time messaging with E2E decryption, reply threading,
/// native file-picker attachment, and clipboard-paste support.
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};
use tokio::sync::mpsc::UnboundedReceiver;

use crate::components::{input_box::InputBox, message_list::MessageList, reply_preview};
use crate::screens::AppAction;
use crate::services::{attachment::format_file_size, websocket::WsEvent};
use crate::theme::Theme;
use crate::types::{DownloadState, LocalMessage, PendingAttachment, WsFrame};

pub struct ChatScreen {
    pub chat_id: i64,
    pub with_user: String,
    pub messages: Vec<LocalMessage>,
    pub my_user_id: i64,
    message_list: MessageList,
    input: InputBox,
    pub reply_to: Option<LocalMessage>,
    pub ws_rx: Option<UnboundedReceiver<WsEvent>>,
    pub ws_connected: bool,
    pub status: String,
    pub pending_send: bool,
    /// Chunked upload progress: (chunks_done, total_chunks).
    pub upload_progress: Option<(usize, usize)>,
    /// File staged for sending (from file picker or clipboard paste).
    pub pending_attachment: Option<PendingAttachment>,
    /// Bounding rect of the [+] attach button, updated each render frame.
    attach_btn_rect: Option<Rect>,
    /// Self-destruct timer in seconds (0 = off).  Ctrl+T cycles through options.
    pub ephemeral_secs: u32,
}

impl ChatScreen {
    pub fn new(chat_id: i64, with_user: impl Into<String>, my_user_id: i64) -> Self {
        Self {
            chat_id,
            with_user: with_user.into(),
            messages: Vec::new(),
            my_user_id,
            message_list: MessageList::new(),
            input: InputBox::new("Type a message…"),
            reply_to: None,
            ws_rx: None,
            ws_connected: false,
            status: String::new(),
            pending_send: false,
            upload_progress: None,
            pending_attachment: None,
            attach_btn_rect: None,
            ephemeral_secs: 0,
        }
    }

    /// Add messages (from REST history or WS) and keep sorted by id.
    pub fn merge_messages(&mut self, new_msgs: Vec<LocalMessage>) {
        for msg in new_msgs {
            if !self.messages.iter().any(|m| m.id == msg.id) {
                self.messages.push(msg);
            }
        }
        self.messages.sort_by_key(|m| m.id);
        // Scroll to bottom whenever new messages arrive
        self.message_list.scroll_to_bottom();
        if !self.messages.is_empty() {
            self.message_list
                .state
                .select(Some(self.messages.len() - 1));
        }
    }

    /// Drain pending WS events. Returns true if new messages were added.
    pub fn poll_ws(&mut self) -> bool {
        // Drain all pending events into a local vec first to release the &mut borrow
        // on ws_rx before calling &mut self methods like merge_messages.
        let events: Vec<WsEvent> = {
            match self.ws_rx.as_mut() {
                None => return false,
                Some(rx) => {
                    let mut buf = Vec::new();
                    while let Ok(ev) = rx.try_recv() {
                        buf.push(ev);
                    }
                    buf
                }
            }
        }; // &mut ws_rx released here

        let mut changed = false;
        for event in events {
            match event {
                WsEvent::Connected => {
                    self.ws_connected = true;
                    self.status = "Connected".to_string();
                }
                WsEvent::Disconnected => {
                    self.ws_connected = false;
                    self.status = "Reconnecting…".to_string();
                }
                WsEvent::Frame(WsFrame::NewMessage { message }) => {
                    if message.sender_id != self.my_user_id {
                        // Native toast notification (Windows Action Center / macOS / Linux)
                        crate::services::notification::notify(&self.with_user);
                        // Also ring the terminal bell as a secondary alert
                        use std::io::Write;
                        let _ = write!(std::io::stderr(), "\x07");
                        let _ = std::io::stderr().flush();
                    }
                    let local = LocalMessage {
                        id: message.id,
                        chat_id: self.chat_id,
                        sender_id: message.sender_id,
                        epoch_id: message.epoch_id,
                        reply_id: message.reply_id,
                        ciphertext: message.ciphertext,
                        nonce: message.nonce,
                        plaintext: None,
                        media_info: None,
                        download_state: DownloadState::None,
                        created_at: message.created_at,
                        synced: true,
                        expires_at: message.expires_at,
                        pixel_preview: None,
                    };
                    self.merge_messages(vec![local]);
                    changed = true;
                }
                WsEvent::Frame(WsFrame::MessageDeleted { message_id }) => {
                    self.messages.retain(|m| m.id != message_id);
                    changed = true;
                }
                WsEvent::Frame(WsFrame::History { .. }) => {}
                WsEvent::Frame(WsFrame::Pong) => {}
                // CallInvite frames are handled by the presence WS in app.rs, not here
                WsEvent::Frame(WsFrame::CallInvite { .. }) => {}
            }
        }
        changed
    }

    /// Returns (action, text_to_send).
    pub fn handle_key(
        &mut self,
        key: KeyEvent,
    ) -> (Option<AppAction>, Option<(String, Option<i64>)>) {
        match key.code {
            // Esc: cancel attachment → cancel reply → go back
            KeyCode::Esc => {
                if self.pending_attachment.is_some() {
                    self.pending_attachment = None;
                    self.status = String::new();
                    return (None, None);
                }
                if self.reply_to.is_some() {
                    self.reply_to = None;
                    return (None, None);
                }
                return (Some(AppAction::Back), None);
            }

            // Enter: send attachment (with optional caption) or plain message
            KeyCode::Enter => {
                if self.pending_send { return (None, None); }

                // Attachment pending — current input is the caption
                if let Some(att) = self.pending_attachment.take() {
                    let caption   = self.input.value.trim().to_string();
                    let reply_id  = self.reply_to.as_ref().map(|m| m.id);
                    self.input.clear();
                    self.reply_to    = None;
                    self.pending_send = true;
                    return (Some(AppAction::SendMedia {
                        path: att.path,
                        caption,
                        reply_id,
                        ephemeral_secs: self.ephemeral_secs,
                    }), None);
                }

                // Plain text message
                if !self.input.value.trim().is_empty() {
                    let text     = self.input.value.trim().to_string();
                    let reply_id = self.reply_to.as_ref().map(|m| m.id);
                    self.input.clear();
                    self.reply_to    = None;
                    self.pending_send = true;
                    return (None, Some((text, reply_id)));
                }
            }

            // Ctrl+O — open native file-picker
            KeyCode::Char('o') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                if !self.pending_send {
                    return (Some(AppAction::OpenFilePicker), None);
                }
            }

            // Ctrl+T — cycle self-destruct timer
            KeyCode::Char('t') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.ephemeral_secs = next_ephemeral(self.ephemeral_secs);
                let label = if self.ephemeral_secs == 0 {
                    "Timer off".to_string()
                } else {
                    format!("Timer: {} (Ctrl+T to change)", fmt_secs(self.ephemeral_secs))
                };
                self.status = label;
                return (None, None);
            }

            // Ctrl+V — paste image / file-path from clipboard
            KeyCode::Char('v') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                // Only intercept when the input is empty to avoid interfering with
                // normal text paste (which crossterm routes as individual Char events)
                if self.input.value.is_empty() && !self.pending_send {
                    return (Some(AppAction::PasteFromClipboard), None);
                }
                // Otherwise let it fall through to the input box handler
                self.input.handle_key(key);
            }

            // Ctrl+D — download selected media message
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                if let Some(idx) = self.message_list.selected_index() {
                    if let Some(msg) = self.messages.get(idx) {
                        if let Some(ref info) = msg.media_info {
                            if !matches!(msg.download_state, DownloadState::Downloaded(_)) {
                                return (Some(AppAction::DownloadMedia {
                                    media_id:   info.media_id,
                                    file_key:   info.file_key.to_vec(),
                                    file_nonce: info.file_nonce.to_vec(),
                                    filename:   info.filename.clone(),
                                }), None);
                            }
                        } else {
                            self.status = "No media attachment on this message".into();
                        }
                    }
                }
            }

            KeyCode::Up => {
                self.message_list.scroll_up();
                self.message_list.move_selection_up(self.messages.len());
            }
            KeyCode::Down => {
                self.message_list.scroll_down();
                self.message_list.move_selection_down(self.messages.len());
            }
            KeyCode::Char('k') if self.input.value.is_empty() => {
                self.message_list.scroll_up();
                self.message_list.move_selection_up(self.messages.len());
            }
            KeyCode::Char('j') if self.input.value.is_empty() => {
                self.message_list.scroll_down();
                self.message_list.move_selection_down(self.messages.len());
            }
            KeyCode::Char('r') | KeyCode::Char('R') if self.input.value.is_empty() => {
                if let Some(idx) = self.message_list.selected_index() {
                    if let Some(msg) = self.messages.get(idx) {
                        self.reply_to = Some(msg.clone() as LocalMessage);
                    }
                }
            }
            KeyCode::Char('s') | KeyCode::Char('S') if self.input.value.is_empty() => {
                return (Some(AppAction::OpenSettings), None);
            }
            _ => {
                self.input.handle_key(key);
            }
        }
        (None, None)
    }

    /// Handle a mouse event.  Returns (action, send_req) — same shape as `handle_key`.
    pub fn handle_mouse(
        &mut self,
        mouse: MouseEvent,
    ) -> (Option<AppAction>, Option<(String, Option<i64>)>) {
        if let MouseEventKind::Down(MouseButton::Left) = mouse.kind {
            if let Some(rect) = self.attach_btn_rect {
                if mouse.column >= rect.x
                    && mouse.column < rect.x + rect.width
                    && mouse.row >= rect.y
                    && mouse.row < rect.y + rect.height
                    && !self.pending_send
                {
                    return (Some(AppAction::OpenFilePicker), None);
                }
            }
        }
        (None, None)
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let ws_indicator = if self.ws_connected { "● " } else { "○ " };
        let ws_color = if self.ws_connected { theme.success } else { theme.error };

        let timer_hint = if self.ephemeral_secs > 0 {
            format!("  ⏳{}  ", fmt_secs(self.ephemeral_secs))
        } else {
            String::new()
        };

        let reply_height: u16 = if self.reply_to.is_some() { 2 } else { 0 };

        // Attachment preview height: pixel rows + 4 overhead lines, capped at 14.
        let attach_height: u16 = match &self.pending_attachment {
            None => 0,
            Some(att) => match &att.pixel_preview {
                Some(pv) => (pv.len() as u16 + 4).min(14),
                None     => 4,
            },
        };

        // Layout: header | messages | reply | attachment-preview | input | status
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),              // 0: header
                Constraint::Min(0),                 // 1: messages
                Constraint::Length(reply_height),   // 2: reply preview
                Constraint::Length(attach_height),  // 3: attachment preview
                Constraint::Length(3),              // 4: input row
                Constraint::Length(1),              // 5: status
            ])
            .split(area);

        // ── Header ────────────────────────────────────────────────────────────
        let header_block = Block::default()
            .borders(Borders::BOTTOM)
            .border_style(Style::default().fg(theme.border));
        let header_text = Line::from(vec![
            Span::styled(ws_indicator, Style::default().fg(ws_color)),
            Span::styled(&self.with_user, Style::default().fg(theme.accent)),
            Span::styled(timer_hint.as_str(), Style::default().fg(theme.error)),
            Span::styled(
                "  ↑↓ scroll  r reply  Ctrl+O attach  Ctrl+V paste  Ctrl+D download  Ctrl+T timer  s settings  Esc back",
                Style::default().fg(theme.muted),
            ),
        ]);
        frame.render_widget(
            Paragraph::new(header_text).block(header_block),
            chunks[0],
        );

        // ── Messages ──────────────────────────────────────────────────────────
        self.message_list.render(
            frame,
            chunks[1],
            &self.messages,
            self.my_user_id,
            &self.with_user,
            theme,
        );

        // ── Reply preview ─────────────────────────────────────────────────────
        if let Some(ref reply) = self.reply_to {
            reply_preview::render(frame, chunks[2], reply, theme);
        }

        // ── Attachment preview ────────────────────────────────────────────────
        if let Some(ref att) = self.pending_attachment {
            render_attachment_preview(frame, chunks[3], att, theme);
        }

        // ── Input row: [+] button | text input ────────────────────────────────
        let input_cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(5), // attach button
                Constraint::Min(0),    // text input
            ])
            .split(chunks[4]);

        // Attach button
        let btn_color = if self.pending_send { theme.muted } else { theme.accent };
        let btn_symbol = if self.pending_attachment.is_some() { "[OK]" } else { "[+] " };
        // Centre the button text vertically (the row is 3 lines tall)
        let btn_y = input_cols[0].y + input_cols[0].height / 2;
        let btn_rect = Rect::new(input_cols[0].x, btn_y, input_cols[0].width, 1);
        frame.render_widget(
            Paragraph::new(btn_symbol)
                .style(Style::default().fg(btn_color))
                .alignment(Alignment::Center),
            btn_rect,
        );
        self.attach_btn_rect = Some(input_cols[0]); // full area for mouse hit-test

        // Input box
        let placeholder: String = if self.pending_attachment.is_some() {
            if self.ephemeral_secs > 0 {
                format!("Add a caption…  Enter to send  [⏳ {}]", fmt_secs(self.ephemeral_secs))
            } else {
                "Add a caption (optional)…  Enter to send".to_string()
            }
        } else if self.ephemeral_secs > 0 {
            format!("Type a message…  [⏳ {}]  Ctrl+T to change", fmt_secs(self.ephemeral_secs))
        } else {
            "Type a message…  Ctrl+O to attach  Ctrl+T timer".to_string()
        };
        self.input.render(frame, input_cols[1], theme, true, &placeholder);

        // ── Status ────────────────────────────────────────────────────────────
        let status_text = if let Some((cur, total)) = self.upload_progress {
            let pct = if total > 0 { cur * 100 / total } else { 0 };
            format!("Uploading… {}/{} ({}%)", cur, total, pct)
        } else {
            self.status.clone()
        };
        frame.render_widget(
            Paragraph::new(status_text.as_str()).style(Style::default().fg(theme.muted)),
            chunks[5],
        );
    }
}

// ── Attachment preview renderer ───────────────────────────────────────────────

/// Cycle through ephemeral timer presets (seconds).
fn next_ephemeral(current: u32) -> u32 {
    match current {
        0       => 10,
        10      => 30,
        30      => 60,
        60      => 300,
        300     => 3600,
        3600    => 86400,
        _       => 0,
    }
}

/// Format seconds for display: "10s", "5m", "1h", "24h".
fn fmt_secs(secs: u32) -> String {
    if secs < 60 {
        format!("{}s", secs)
    } else if secs < 3600 {
        let m = secs / 60;
        let s = secs % 60;
        if s == 0 { format!("{}m", m) } else { format!("{}m{}s", m, s) }
    } else {
        format!("{}h", secs / 3600)
    }
}

fn render_attachment_preview(frame: &mut Frame, area: Rect, att: &PendingAttachment, theme: &Theme) {
    if area.height == 0 { return; }

    let outer_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.accent))
        .title(Span::styled(" Attachment ", Style::default().fg(theme.accent)));
    let inner = outer_block.inner(area);
    frame.render_widget(outer_block, area);

    if inner.height == 0 { return; }

    // Split inner: [preview content | file-info line | hint line]
    let info_h = 1u16;
    let hint_h = 1u16;
    let content_h = inner.height.saturating_sub(info_h + hint_h);

    let splits = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(content_h),
            Constraint::Length(info_h),
            Constraint::Length(hint_h),
        ])
        .split(inner);

    // ── Content (pixel preview or icon) ──────────────────────────────────────
    if let Some(ref pv) = att.pixel_preview {
        // Render image as coloured Unicode half-blocks
        let rlines: Vec<Line> = pv
            .iter()
            .take(content_h as usize)
            .map(|row| {
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
                Line::from(spans)
            })
            .collect();
        frame.render_widget(Paragraph::new(rlines), splits[0]);
    } else {
        let icon = match att.file_type.as_str() {
            "video"    => "[video]",
            "audio"    => "[audio]",
            "document" => "[doc]  ",
            _          => "[file] ",
        };
        frame.render_widget(
            Paragraph::new(format!("{} {}", icon, att.filename))
                .style(Style::default().fg(theme.accent)),
            splits[0],
        );
    }

    // ── File info ─────────────────────────────────────────────────────────────
    frame.render_widget(
        Paragraph::new(format!(
            "{} · {} · {}",
            att.filename,
            format_file_size(att.file_size),
            att.file_type
        ))
        .style(Style::default().fg(theme.muted)),
        splits[1],
    );

    // ── Hint ──────────────────────────────────────────────────────────────────
    frame.render_widget(
        Paragraph::new("Enter = send  Esc = cancel attachment  type to add caption")
            .style(Style::default().fg(theme.muted)),
        splits[2],
    );
}
