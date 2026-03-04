//! Full-screen VoIP call screen.
//!
//! Layout:
//! ```text
//! ┌─ CALL WITH alice ──────────────────────────────────────────────────┐
//! │  ⏱ 00:03:42            ● Connected    [H] on hold                  │
//! ├────────────────────────────────────────────────────────────────────┤
//! │  [A] Answer   [R] Reject                 (ringing state only)      │
//! │  [M] Mute     [H] Hold    [E] End         (active state)           │
//! ├─ NOISE FILTERS ───────────────────────────────────────────────────┤
//! │  [1] Quiet room  [2] Office  [3] Outdoor  [4] Heavy noise          │
//! │  > Suppression  ████████░░  80%                                    │
//! │    Gate         ██████░░░░ -45 dB                                  │
//! │    High-pass    █████░░░░░  200 Hz                                  │
//! ├────────────────────────────────────────────────────────────────────┤
//! │  Mic  ▁▂▄▆█▆▄▂▁  Peer ▁▁▂▃▅▃▂▁▁                                   │
//! └────────────────────────────────────────────────────────────────────┘
//! ```

use std::time::Instant;

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Gauge, Paragraph},
    Frame,
};

use crate::screens::AppAction;
use crate::theme::Theme;
use crate::types::{CallState, FilterParams, FilterPreset};

// ── Focus state ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Focus {
    Controls,
    FilterPresetRow,
    SliderSuppression,
    SliderGate,
    SliderHighpass,
}

// ── CallScreen ────────────────────────────────────────────────────────────────

pub struct CallScreen {
    pub call_state:  CallState,
    pub filter:      FilterParams,
    pub muted:       bool,
    pub held:        bool,
    /// Local microphone RMS level in range [0.0, 1.0].
    pub local_level: f32,
    /// Remote audio RMS level in range [0.0, 1.0].
    pub remote_level: f32,
    /// When the call transitioned to `Active` — used for the call timer.
    pub call_start:  Option<Instant>,
    /// Status line shown at the bottom.
    pub status:      String,
    focus:           Focus,
}

impl CallScreen {
    pub fn new(call_state: CallState) -> Self {
        Self {
            call_state,
            filter:       FilterParams::default(),
            muted:        false,
            held:         false,
            local_level:  0.0,
            remote_level: 0.0,
            call_start:   None,
            status:       String::new(),
            focus:        Focus::Controls,
        }
    }

    // ── Key handling ──────────────────────────────────────────────────────────

    /// Returns an optional `AppAction` when navigation is needed, plus an
    /// optional updated `FilterParams` when the slider changed.
    pub fn handle_key(&mut self, key: KeyEvent) -> (Option<AppAction>, Option<FilterParams>) {
        match self.focus {
            Focus::Controls => self.handle_controls_key(key),
            Focus::FilterPresetRow => self.handle_preset_key(key),
            Focus::SliderSuppression |
            Focus::SliderGate        |
            Focus::SliderHighpass    => self.handle_slider_key(key),
        }
    }

    fn handle_controls_key(&mut self, key: KeyEvent) -> (Option<AppAction>, Option<FilterParams>) {
        match key.code {
            // Answer / Reject (ringing)
            KeyCode::Char('a') | KeyCode::Char('A') => {
                if let CallState::Ringing { call_id, .. } = &self.call_state {
                    return (Some(AppAction::AnswerCall { call_id: call_id.clone() }), None);
                }
            }
            KeyCode::Char('r') | KeyCode::Char('R') => {
                if let CallState::Ringing { call_id, .. } = &self.call_state {
                    return (Some(AppAction::RejectCall { call_id: call_id.clone() }), None);
                }
                if let CallState::Calling { call_id, .. } = &self.call_state {
                    // Caller cancels
                    return (Some(AppAction::RejectCall { call_id: call_id.clone() }), None);
                }
            }
            // Mute toggle
            KeyCode::Char('m') | KeyCode::Char('M') => {
                if matches!(self.call_state, CallState::Active { .. }) {
                    self.muted = !self.muted;
                    self.status = if self.muted { "Muted".into() } else { String::new() };
                }
            }
            // Hold toggle
            KeyCode::Char('h') | KeyCode::Char('H') => {
                if matches!(self.call_state, CallState::Active { .. }) {
                    self.held = !self.held;
                    self.status = if self.held { "On hold".into() } else { String::new() };
                }
            }
            // End call
            KeyCode::Char('e') | KeyCode::Char('E') | KeyCode::Char('q') => {
                return (Some(AppAction::EndCall), None);
            }
            // Navigate into filter section
            KeyCode::Down | KeyCode::Tab => {
                self.focus = Focus::FilterPresetRow;
            }
            _ => {}
        }
        (None, None)
    }

    fn handle_preset_key(&mut self, key: KeyEvent) -> (Option<AppAction>, Option<FilterParams>) {
        match key.code {
            KeyCode::Char('1') => {
                self.filter = FilterPreset::QuietRoom.params();
                return (None, Some(self.filter.clone()));
            }
            KeyCode::Char('2') => {
                self.filter = FilterPreset::Office.params();
                return (None, Some(self.filter.clone()));
            }
            KeyCode::Char('3') => {
                self.filter = FilterPreset::Outdoor.params();
                return (None, Some(self.filter.clone()));
            }
            KeyCode::Char('4') => {
                self.filter = FilterPreset::HeavyNoise.params();
                return (None, Some(self.filter.clone()));
            }
            KeyCode::Down | KeyCode::Tab => {
                self.focus = Focus::SliderSuppression;
            }
            KeyCode::Up | KeyCode::BackTab => {
                self.focus = Focus::Controls;
            }
            KeyCode::Char('q') | KeyCode::Char('e') => {
                return (Some(AppAction::EndCall), None);
            }
            _ => {}
        }
        (None, None)
    }

    fn handle_slider_key(&mut self, key: KeyEvent) -> (Option<AppAction>, Option<FilterParams>) {
        self.filter.preset = FilterPreset::Custom;
        let changed = match key.code {
            KeyCode::Left  => { self.adjust_slider(-1); true }
            KeyCode::Right => { self.adjust_slider( 1); true }
            KeyCode::Down | KeyCode::Tab => {
                self.focus = match self.focus {
                    Focus::SliderSuppression => Focus::SliderGate,
                    Focus::SliderGate        => Focus::SliderHighpass,
                    Focus::SliderHighpass    => Focus::Controls,
                    _ => Focus::Controls,
                };
                false
            }
            KeyCode::Up | KeyCode::BackTab => {
                self.focus = match self.focus {
                    Focus::SliderSuppression => Focus::FilterPresetRow,
                    Focus::SliderGate        => Focus::SliderSuppression,
                    Focus::SliderHighpass    => Focus::SliderGate,
                    _ => Focus::FilterPresetRow,
                };
                false
            }
            KeyCode::Char('q') | KeyCode::Char('e') => {
                return (Some(AppAction::EndCall), None);
            }
            _ => { false }
        };

        let update = if changed { Some(self.filter.clone()) } else { None };
        (None, update)
    }

    fn adjust_slider(&mut self, delta: i32) {
        match self.focus {
            Focus::SliderSuppression => {
                self.filter.suppression =
                    (self.filter.suppression + delta as f32 * 0.05).clamp(0.0, 1.0);
            }
            Focus::SliderGate => {
                self.filter.gate_db =
                    (self.filter.gate_db + delta as f32 * 1.0).clamp(-80.0, 0.0);
            }
            Focus::SliderHighpass => {
                self.filter.highpass_hz =
                    (self.filter.highpass_hz + delta as f32 * 20.0).clamp(20.0, 2000.0);
            }
            _ => {}
        }
    }

    // ── Render ────────────────────────────────────────────────────────────────

    pub fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),  // header + timer
                Constraint::Length(3),  // controls row
                Constraint::Length(6),  // noise filters
                Constraint::Length(3),  // VU meters
                Constraint::Length(1),  // status bar
            ])
            .split(area);

        self.render_header(frame, chunks[0], theme);
        self.render_controls(frame, chunks[1], theme);
        self.render_filters(frame, chunks[2], theme);
        self.render_vu_meters(frame, chunks[3], theme);

        // Status bar
        frame.render_widget(
            Paragraph::new(self.status.as_str())
                .style(Style::default().fg(theme.muted)),
            chunks[4],
        );
    }

    fn render_header(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let (peer, state_str, state_color) = match &self.call_state {
            CallState::Ringing { caller, .. } => (
                caller.as_str(),
                " ↙ INCOMING CALL ",
                theme.accent,
            ),
            CallState::Calling { peer, .. } => (
                peer.as_str(),
                " ↗ CALLING ",
                theme.muted,
            ),
            CallState::Active { peer, .. } => (
                peer.as_str(),
                " ● CONNECTED ",
                theme.success,
            ),
            CallState::Ended { .. } => (
                "",
                " ENDED ",
                theme.error,
            ),
            CallState::Idle => ("", " IDLE ", theme.muted),
        };

        // Build timer string for active calls
        let timer = if let Some(start) = self.call_start {
            let secs = start.elapsed().as_secs();
            format!(" ⏱ {:02}:{:02}:{:02}  ", secs / 3600, (secs % 3600) / 60, secs % 60)
        } else {
            String::new()
        };

        let mut spans = vec![
            Span::styled(
                format!(" CALL WITH {} ", peer.to_uppercase()),
                Style::default().fg(theme.accent).add_modifier(Modifier::BOLD),
            ),
            Span::styled(state_str, Style::default().fg(state_color)),
        ];
        if !timer.is_empty() {
            spans.push(Span::styled(timer, Style::default().fg(theme.muted)));
        }
        if self.held {
            spans.push(Span::styled(" [on hold] ", Style::default().fg(theme.unread)));
        }
        if self.muted {
            spans.push(Span::styled(" [muted] ", Style::default().fg(theme.unread)));
        }

        frame.render_widget(
            Paragraph::new(Line::from(spans))
                .block(Block::default()
                    .borders(Borders::BOTTOM)
                    .border_style(Style::default().fg(theme.border))),
            area,
        );
    }

    fn render_controls(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let is_ringing = matches!(&self.call_state, CallState::Ringing { .. });
        let is_calling = matches!(&self.call_state, CallState::Calling { .. });
        let is_active  = matches!(&self.call_state, CallState::Active  { .. });

        let focused  = self.focus == Focus::Controls;
        let hi_style = Style::default()
            .fg(theme.accent)
            .add_modifier(if focused { Modifier::BOLD | Modifier::UNDERLINED } else { Modifier::empty() });
        let dim_style = Style::default().fg(theme.muted);

        let text: Line = if is_ringing {
            Line::from(vec![
                Span::styled("  [A] Answer  ", hi_style),
                Span::styled("[R] Reject  ", dim_style),
                Span::styled("  ← ↑↓ → sliders  ", dim_style),
            ])
        } else if is_calling {
            Line::from(vec![
                Span::styled("  Waiting for answer…  ", dim_style),
                Span::styled("[R] Cancel call  ", hi_style),
            ])
        } else if is_active {
            let mute_style = if self.muted { Style::default().fg(theme.unread) } else { hi_style };
            let hold_style = if self.held  { Style::default().fg(theme.unread) } else { hi_style };
            Line::from(vec![
                Span::styled("  [M] ", mute_style),
                Span::styled(if self.muted { "Unmute  " } else { "Mute    " }, mute_style),
                Span::styled("[H] ", hold_style),
                Span::styled(if self.held { "Unhold  " } else { "Hold    " }, hold_style),
                Span::styled("[E] End call  ", Style::default().fg(theme.error)),
                Span::styled("  ↑↓ sliders  Tab navigate  ", dim_style),
            ])
        } else {
            Line::from(vec![Span::styled("  Call ended.  [E] Back", dim_style)])
        };

        frame.render_widget(
            Paragraph::new(text)
                .block(Block::default()
                    .borders(Borders::BOTTOM)
                    .border_style(Style::default().fg(theme.border))),
            area,
        );
    }

    fn render_filters(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let filter_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1), // title + presets
                Constraint::Length(1), // suppression slider
                Constraint::Length(1), // gate slider
                Constraint::Length(1), // highpass slider
                Constraint::Min(0),
            ])
            .split(area);

        // ── Preset row ────────────────────────────────────────────────────────
        let preset_focused = self.focus == Focus::FilterPresetRow;
        let active_preset = self.filter.preset;

        let preset_span = |p: FilterPreset, label: &str| {
            let active = p == active_preset;
            let style = if active {
                Style::default()
                    .fg(theme.accent)
                    .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
            } else if preset_focused {
                Style::default().fg(theme.muted)
            } else {
                Style::default().fg(theme.muted)
            };
            let n = match p {
                FilterPreset::QuietRoom  => "1",
                FilterPreset::Office     => "2",
                FilterPreset::Outdoor    => "3",
                FilterPreset::HeavyNoise => "4",
                FilterPreset::Custom     => "C",
            };
            Span::styled(format!(" [{}]{} ", n, label), style)
        };

        let border_style = if preset_focused {
            Style::default().fg(theme.accent)
        } else {
            Style::default().fg(theme.border)
        };

        let preset_line = Line::from(vec![
            Span::styled(" Filters: ", Style::default().fg(theme.muted)),
            preset_span(FilterPreset::QuietRoom,  "Quiet"),
            preset_span(FilterPreset::Office,     "Office"),
            preset_span(FilterPreset::Outdoor,    "Outdoor"),
            preset_span(FilterPreset::HeavyNoise, "Heavy"),
        ]);
        frame.render_widget(
            Paragraph::new(preset_line).style(border_style),
            filter_chunks[0],
        );

        // ── Sliders ───────────────────────────────────────────────────────────
        self.render_slider(
            frame, filter_chunks[1], theme,
            "Suppression",
            self.filter.suppression,
            0.0, 1.0,
            &format!("{:3.0}%", self.filter.suppression * 100.0),
            self.focus == Focus::SliderSuppression,
        );
        self.render_slider(
            frame, filter_chunks[2], theme,
            "Gate dB    ",
            (self.filter.gate_db + 80.0) / 80.0,  // normalize to 0..1
            0.0, 1.0,
            &format!("{:5.0} dB", self.filter.gate_db),
            self.focus == Focus::SliderGate,
        );
        self.render_slider(
            frame, filter_chunks[3], theme,
            "High-pass  ",
            (self.filter.highpass_hz - 20.0) / (2000.0 - 20.0),
            0.0, 1.0,
            &format!("{:4.0} Hz", self.filter.highpass_hz),
            self.focus == Focus::SliderHighpass,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn render_slider(
        &self,
        frame: &mut Frame,
        area: Rect,
        theme: &Theme,
        label: &str,
        value_norm: f32,
        _min: f32,
        _max: f32,
        value_str: &str,
        focused: bool,
    ) {
        let label_width = 13u16;
        let val_width   = 8u16;
        let bar_width   = area.width.saturating_sub(label_width + val_width + 4);

        let inner = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(label_width),
                Constraint::Length(bar_width),
                Constraint::Length(val_width),
            ])
            .split(area);

        let label_style = if focused {
            Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme.muted)
        };
        let cursor = if focused { " ▶ " } else { "   " };

        frame.render_widget(
            Paragraph::new(format!("{}{}", cursor, label))
                .style(label_style),
            inner[0],
        );

        frame.render_widget(
            Gauge::default()
                .gauge_style(Style::default()
                    .fg(if focused { theme.accent } else { theme.border })
                    .bg(theme.surface))
                .ratio(value_norm.clamp(0.0, 1.0) as f64),
            inner[1],
        );

        frame.render_widget(
            Paragraph::new(value_str).style(Style::default().fg(theme.muted)),
            inner[2],
        );
    }

    fn render_vu_meters(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let halves = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(area);

        let level_bar = |level: f32| -> String {
            // 9-bar block graph using Unicode block elements
            let bars = ["▁", "▂", "▃", "▄", "▅", "▆", "▇", "█"];
            let n = (level * 16.0).clamp(0.0, 16.0) as usize;
            let full = n / 2;
            let half = n % 2;
            let mut s = String::new();
            for i in 0..8 {
                if i < full {
                    s.push_str("█");
                } else if i == full && half == 1 {
                    s.push_str(bars[3]);
                } else {
                    s.push('░');
                }
            }
            s
        };

        frame.render_widget(
            Gauge::default()
                .block(Block::default()
                    .title(format!("Mic  {}", level_bar(self.local_level)))
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme.border)))
                .gauge_style(Style::default().fg(theme.accent).bg(theme.surface))
                .ratio(self.local_level.clamp(0.0, 1.0) as f64),
            halves[0],
        );

        frame.render_widget(
            Gauge::default()
                .block(Block::default()
                    .title(format!("Peer {}", level_bar(self.remote_level)))
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme.border)))
                .gauge_style(Style::default().fg(theme.success).bg(theme.surface))
                .ratio(self.remote_level.clamp(0.0, 1.0) as f64),
            halves[1],
        );
    }
}
