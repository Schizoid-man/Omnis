/// Home screen — chat list with search and new-chat prompt.
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::Style,
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use crate::components::{chat_list::ChatList, input_box::InputBox};
use crate::screens::AppAction;
use crate::theme::Theme;
use crate::types::LocalChat;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Focus {
    List,
    Search,
    NewChat,
}

pub struct HomeScreen {
    pub chat_list: ChatList,
    search: InputBox,
    new_chat_input: InputBox,
    focus: Focus,
    pub status: String,
}

impl HomeScreen {
    pub fn new() -> Self {
        Self {
            chat_list: ChatList::new(),
            search: InputBox::new("search chats…"),
            new_chat_input: InputBox::new("username to start chat…"),
            focus: Focus::List,
            status: String::new(),
        }
    }

    /// Filter chats by search query.
    pub fn visible_chats<'a>(&self, chats: &'a [LocalChat]) -> Vec<&'a LocalChat> {
        let q = self.search.value.to_lowercase();
        if q.is_empty() {
            chats.iter().collect()
        } else {
            chats
                .iter()
                .filter(|c| c.with_user.to_lowercase().contains(&q))
                .collect()
        }
    }

    /// Returns Some(AppAction) when the screen wants to navigate.
    pub fn handle_key(&mut self, key: KeyEvent, chats: &[LocalChat]) -> Option<AppAction> {
        match self.focus {
            Focus::Search => match key.code {
                KeyCode::Esc => {
                    self.search.clear();
                    self.focus = Focus::List;
                    None
                }
                KeyCode::Enter => {
                    self.focus = Focus::List;
                    None
                }
                _ => {
                    self.search.handle_key(key);
                    // reset selection when search changes
                    let vis = self.visible_chats(chats);
                    if !vis.is_empty() {
                        self.chat_list.select(0);
                    }
                    None
                }
            },
            Focus::NewChat => match key.code {
                KeyCode::Esc => {
                    self.new_chat_input.clear();
                    self.focus = Focus::List;
                    None
                }
                KeyCode::Enter => {
                    let username = self.new_chat_input.value.trim().to_string();
                    self.new_chat_input.clear();
                    self.focus = Focus::List;
                    if username.is_empty() {
                        None
                    } else {
                        // App will handle the actual create_chat call
                        self.status = format!("Opening chat with {}…", username);
                        Some(AppAction::OpenChat { chat_id: -1, with_user: username })
                    }
                }
                _ => {
                    self.new_chat_input.handle_key(key);
                    None
                }
            },
            Focus::List => match key.code {
                KeyCode::Char('q') | KeyCode::Char('Q') => Some(AppAction::Quit),
                KeyCode::Char('s') | KeyCode::Char('S') => Some(AppAction::OpenSettings),
                KeyCode::Char('p') | KeyCode::Char('P') => Some(AppAction::OpenProfile),
                KeyCode::Char('/') => {
                    self.focus = Focus::Search;
                    None
                }
                KeyCode::Char('n') | KeyCode::Char('N') => {
                    self.focus = Focus::NewChat;
                    None
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    let vis = self.visible_chats(chats);
                    self.chat_list.move_up(vis.len());
                    None
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    let vis = self.visible_chats(chats);
                    self.chat_list.move_down(vis.len());
                    None
                }
                KeyCode::Enter => {
                    let vis = self.visible_chats(chats);
                    if let Some(idx) = self.chat_list.selected() {
                        if let Some(chat) = vis.get(idx) {
                            return Some(AppAction::OpenChat {
                                chat_id: chat.chat_id,
                                with_user: chat.with_user.clone(),
                            });
                        }
                    }
                    None
                }
                KeyCode::Char('c') | KeyCode::Char('C') => {
                    let vis = self.visible_chats(chats);
                    if let Some(idx) = self.chat_list.selected() {
                        if let Some(chat) = vis.get(idx) {
                            self.status = format!("Calling {}…", chat.with_user);
                            return Some(AppAction::InitiateCall {
                                peer_username: chat.with_user.clone(),
                            });
                        }
                    }
                    None
                }
                _ => None,
            },
        }
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect, chats: &[LocalChat], theme: &Theme) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // header
                Constraint::Length(3), // search bar
                Constraint::Min(0),    // chat list
                Constraint::Length(if self.focus == Focus::NewChat { 3 } else { 1 }), // new chat / hint
                Constraint::Length(1), // status
            ])
            .split(area);

        // Header
        frame.render_widget(
            Paragraph::new(" Omnis")
                .style(Style::default().fg(theme.accent))
                .block(Block::default().borders(Borders::BOTTOM).border_style(Style::default().fg(theme.border))),
            chunks[0],
        );

        // Search
        self.search.render(frame, chunks[1], theme, self.focus == Focus::Search, "Search  /");

        // Chat list
        let vis: Vec<LocalChat> = self
            .visible_chats(chats)
            .into_iter()
            .cloned()
            .collect();
        self.chat_list.render(frame, chunks[2], &vis, theme);

        // New chat input or hint
        if self.focus == Focus::NewChat {
            self.new_chat_input.render(
                frame,
                chunks[3],
                theme,
                true,
                "New Chat  n",
            );
        } else {
            frame.render_widget(
                Paragraph::new(" n new  /search  c call  s settings  p profile  q quit")
                    .style(Style::default().fg(theme.muted)),
                chunks[3],
            );
        }

        // Status
        frame.render_widget(
            Paragraph::new(self.status.as_str()).style(Style::default().fg(theme.muted)),
            chunks[4],
        );
    }
}
