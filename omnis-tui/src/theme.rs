use ratatui::style::Color;

/// Parse a hex colour string like "#6C63FF" or "6C63FF" into a ratatui Color.
/// Falls back to a default purple if the string is invalid.
pub fn parse_hex_color(hex: &str) -> Color {
    let hex = hex.trim_start_matches('#');
    if hex.len() == 6 {
        if let (Ok(r), Ok(g), Ok(b)) = (
            u8::from_str_radix(&hex[0..2], 16),
            u8::from_str_radix(&hex[2..4], 16),
            u8::from_str_radix(&hex[4..6], 16),
        ) {
            return Color::Rgb(r, g, b);
        }
    }
    Color::Rgb(108, 99, 255) // default purple
}

#[derive(Debug, Clone)]
pub struct Theme {
    pub accent: Color,
    pub surface: Color,
    pub border: Color,
    pub muted: Color,
    pub text: Color,
    pub error: Color,
    pub success: Color,
    pub unread: Color,
}

impl Theme {
    pub fn from_hex(hex: &str) -> Self {
        Self {
            accent: parse_hex_color(hex),
            surface: Color::Rgb(18, 18, 18),
            border: Color::Rgb(50, 50, 60),
            muted: Color::Rgb(120, 120, 140),
            text: Color::Rgb(220, 220, 220),
            error: Color::Rgb(255, 80, 80),
            success: Color::Rgb(80, 200, 120),
            unread: Color::Rgb(255, 200, 60),
        }
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::from_hex("#6C63FF")
    }
}
