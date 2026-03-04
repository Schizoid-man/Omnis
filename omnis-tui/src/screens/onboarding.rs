/// Onboarding screen: API URL config, then login or signup.
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::Span,
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
    Frame,
};

use crate::components::input_box::InputBox;
use crate::screens::AppAction;
use crate::theme::Theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Login,
    Signup,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Field {
    ApiUrl,
    TestButton,
    Username,
    Password,
    ConfirmPassword,
}

pub struct OnboardingScreen {
    mode: Mode,
    focused: Field,
    api_url: InputBox,
    username: InputBox,
    password: InputBox,
    confirm: InputBox,
    pub status: String,
    loading: bool,
}

impl OnboardingScreen {
    pub fn new(api_url: &str) -> Self {
        let mut api_input = InputBox::new("http://localhost:8000");
        api_input.set_value(api_url);
        Self {
            mode: Mode::Login,
            focused: Field::ApiUrl,
            api_url: api_input,
            username: InputBox::new("username"),
            password: { let mut b = InputBox::new("password"); b.secret = true; b },
            confirm: { let mut b = InputBox::new("confirm password"); b.secret = true; b },
            status: String::new(),
            loading: false,
        }
    }

    fn next_field(&mut self) {
        self.focused = match (&self.mode, &self.focused) {
            (_, Field::ApiUrl) => Field::TestButton,
            (_, Field::TestButton) => Field::Username,
            (_, Field::Username) => Field::Password,
            (Mode::Signup, Field::Password) => Field::ConfirmPassword,
            _ => Field::ApiUrl,
        };
    }

    fn prev_field(&mut self) {
        self.focused = match (&self.mode, &self.focused) {
            (_, Field::TestButton) => Field::ApiUrl,
            (_, Field::Username) => Field::TestButton,
            (_, Field::Password) => Field::Username,
            (Mode::Signup, Field::ConfirmPassword) => Field::Password,
            _ => Field::ApiUrl,
        };
    }

    fn active_input_mut(&mut self) -> &mut InputBox {
        match self.focused {
            Field::ApiUrl | Field::TestButton => &mut self.api_url,
            Field::Username => &mut self.username,
            Field::Password => &mut self.password,
            Field::ConfirmPassword => &mut self.confirm,
        }
    }

    /// Returns true when the user hits Enter on the Test Connection button.
    pub fn is_test_ready(&self, key: &KeyEvent) -> bool {
        self.focused == Field::TestButton && key.code == KeyCode::Enter && !self.loading
    }

    /// Current value of the API URL field.
    pub fn api_url_value(&self) -> String {
        self.api_url.value.clone()
    }

    /// Handle a key event. Returns Some if an async action must be run.
    /// Callers must call `submit()` after Enter on the last field.
    pub fn handle_key(&mut self, key: KeyEvent) -> Option<AppAction> {
        if self.loading {
            return None;
        }
        match key.code {
            KeyCode::Tab => {
                self.next_field();
                None
            }
            KeyCode::BackTab => {
                self.prev_field();
                None
            }
            KeyCode::F(1) => {
                self.mode = match self.mode {
                    Mode::Login => Mode::Signup,
                    Mode::Signup => Mode::Login,
                };
                self.status.clear();
                None
            }
            KeyCode::Esc => Some(AppAction::Quit),
            KeyCode::Enter => {
                // Enter on TestButton is handled by app.rs via is_test_ready()
                if self.focused == Field::TestButton {
                    return None;
                }
                // Enter on last field = submit
                let is_last = match (&self.mode, &self.focused) {
                    (Mode::Login, Field::Password) => true,
                    (Mode::Signup, Field::ConfirmPassword) => true,
                    _ => false,
                };
                if is_last {
                    None // App will call submit() on the tokio side
                } else {
                    self.next_field();
                    None
                }
            }
            _ => {
                // Only forward to input when actually on an input field
                if self.focused != Field::TestButton {
                    self.active_input_mut().handle_key(key);
                }
                None
            }
        }
    }

    /// Returns true if the form is ready to submit (Enter pressed on last field).
    pub fn is_submit_key(key: &KeyEvent, mode_is_login: bool, focused: Field) -> bool {
        key.code == KeyCode::Enter
            && ((mode_is_login && focused == Field::Password)
                || (!mode_is_login && focused == Field::ConfirmPassword))
    }

    pub fn is_submit_ready(&self, key: &KeyEvent) -> bool {
        Self::is_submit_key(key, self.mode == Mode::Login, self.focused)
    }

    pub fn is_login_mode(&self) -> bool {
        self.mode == Mode::Login
    }

    pub fn current_field(&self) -> Field {
        self.focused
    }

    pub fn set_status(&mut self, s: impl Into<String>) {
        self.status = s.into();
        self.loading = false;
    }

    pub fn set_loading(&mut self) {
        self.loading = true;
        self.status = "Working…".to_string();
    }

    /// Extract form values for the async submit call.
    pub fn form_values(&self) -> (String, String, String, String) {
        (
            self.api_url.value.clone(),
            self.username.value.clone(),
            self.password.value.clone(),
            self.confirm.value.clone(),
        )
    }

    pub fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        frame.render_widget(Clear, area);

        let title = match self.mode {
            Mode::Login => " Omnis — Login ",
            Mode::Signup => " Omnis — Sign Up ",
        };

        let outer = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.accent))
            .title(Span::styled(title, Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)));

        let inner = outer.inner(area);
        frame.render_widget(outer, area);

        // Layout:
        //  [API URL   3]
        //  [Test btn  3]
        //  [spacer    1]
        //  [Username  3]
        //  [Password  3]
        //  [Confirm   3]  (signup only)
        //  [spacer    1]
        //  [status    4]  (wrapped, multiline)
        //  [hint      1]
        let form_width = inner.width.min(56);
        let form_height: u16 = if self.mode == Mode::Login { 19 } else { 22 };
        let x = inner.x + inner.width.saturating_sub(form_width) / 2;
        let y = inner.y + inner.height.saturating_sub(form_height) / 2;
        let form_area = Rect::new(x, y, form_width, form_height.min(inner.height));

        let mut constraints = vec![
            Constraint::Length(3), // 0: api url
            Constraint::Length(3), // 1: test button
            Constraint::Length(1), // 2: spacer
            Constraint::Length(3), // 3: username
            Constraint::Length(3), // 4: password
        ];
        if self.mode == Mode::Signup {
            constraints.push(Constraint::Length(3)); // 5: confirm
        }
        constraints.push(Constraint::Length(1)); // spacer
        constraints.push(Constraint::Length(4)); // status (multiline)
        constraints.push(Constraint::Length(1)); // hint

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints(constraints)
            .split(form_area);

        // API URL
        self.api_url.render(frame, chunks[0], theme, self.focused == Field::ApiUrl, "API URL");

        // Test Connection button
        let test_focused = self.focused == Field::TestButton;
        let test_label = if self.loading && test_focused {
            "  Testing…"
        } else {
            "  Test Connection"
        };
        let btn_style = if test_focused {
            Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme.muted)
        };
        let btn_block = Block::default()
            .borders(Borders::ALL)
            .border_style(btn_style)
            .title(Span::styled(" Action ", Style::default().fg(theme.muted)));
        frame.render_widget(
            Paragraph::new(test_label).style(btn_style).block(btn_block),
            chunks[1],
        );

        // Username / Password
        self.username.render(frame, chunks[3], theme, self.focused == Field::Username, "Username");
        self.password.render(frame, chunks[4], theme, self.focused == Field::Password, "Password");

        let mut offset = 5usize;
        if self.mode == Mode::Signup {
            self.confirm.render(
                frame, chunks[offset], theme,
                self.focused == Field::ConfirmPassword, "Confirm Password",
            );
            offset += 1;
        }

        // spacer is chunks[offset], skip it
        offset += 1;

        // Status — always 4 rows, text wrapped so long errors are fully visible
        let is_error = self.status.starts_with("Error")
            || self.status.contains("fail")
            || self.status.contains("✗");
        let is_ok = self.status.contains("OK") || self.status.contains("✓") || self.status.contains("Success");
        let status_style = if is_error {
            Style::default().fg(theme.error)
        } else if is_ok {
            Style::default().fg(theme.success)
        } else {
            Style::default().fg(theme.muted)
        };
        frame.render_widget(
            Paragraph::new(self.status.as_str())
                .style(status_style)
                .alignment(Alignment::Center)
                .wrap(Wrap { trim: false }),
            chunks[offset],
        );
        offset += 1;

        // Hint
        let hint = "F1 toggle mode  |  Tab navigate  |  Enter select/submit  |  Esc quit";
        frame.render_widget(
            Paragraph::new(hint)
                .style(Style::default().fg(theme.muted))
                .alignment(Alignment::Center),
            chunks[offset],
        );
    }
}
