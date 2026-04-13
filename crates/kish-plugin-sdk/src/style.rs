/// Terminal colors for prompt styling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Color {
    Black,
    Red,
    Green,
    Yellow,
    Blue,
    Magenta,
    Cyan,
    White,
    /// 256-color palette index (0-255).
    Fixed(u8),
    /// 24-bit RGB color.
    Rgb(u8, u8, u8),
}

/// Text style builder for generating ANSI escape sequences.
#[derive(Debug, Clone, Default)]
pub struct Style {
    fg: Option<Color>,
    bg: Option<Color>,
    bold: bool,
    dim: bool,
    italic: bool,
    underline: bool,
}

fn fg_code(color: &Color) -> String {
    match color {
        Color::Black => "30".into(),
        Color::Red => "31".into(),
        Color::Green => "32".into(),
        Color::Yellow => "33".into(),
        Color::Blue => "34".into(),
        Color::Magenta => "35".into(),
        Color::Cyan => "36".into(),
        Color::White => "37".into(),
        Color::Fixed(n) => format!("38;5;{n}"),
        Color::Rgb(r, g, b) => format!("38;2;{r};{g};{b}"),
    }
}

fn bg_code(color: &Color) -> String {
    match color {
        Color::Black => "40".into(),
        Color::Red => "41".into(),
        Color::Green => "42".into(),
        Color::Yellow => "43".into(),
        Color::Blue => "44".into(),
        Color::Magenta => "45".into(),
        Color::Cyan => "46".into(),
        Color::White => "47".into(),
        Color::Fixed(n) => format!("48;5;{n}"),
        Color::Rgb(r, g, b) => format!("48;2;{r};{g};{b}"),
    }
}

impl Style {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn fg(mut self, color: Color) -> Self {
        self.fg = Some(color);
        self
    }

    pub fn bg(mut self, color: Color) -> Self {
        self.bg = Some(color);
        self
    }

    pub fn bold(mut self) -> Self {
        self.bold = true;
        self
    }

    pub fn dim(mut self) -> Self {
        self.dim = true;
        self
    }

    pub fn italic(mut self) -> Self {
        self.italic = true;
        self
    }

    pub fn underline(mut self) -> Self {
        self.underline = true;
        self
    }

    /// Wrap `text` in ANSI escape codes for this style.
    /// Appends a reset after the text. Returns text unchanged if no style set.
    pub fn paint(&self, text: &str) -> String {
        let mut codes: Vec<String> = Vec::new();
        if self.bold { codes.push("1".into()); }
        if self.dim { codes.push("2".into()); }
        if self.italic { codes.push("3".into()); }
        if self.underline { codes.push("4".into()); }
        if let Some(ref fg) = self.fg { codes.push(fg_code(fg)); }
        if let Some(ref bg) = self.bg { codes.push(bg_code(bg)); }
        if codes.is_empty() {
            return text.to_string();
        }
        format!("\x1b[{}m{}\x1b[0m", codes.join(";"), text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_style_passthrough() {
        assert_eq!(Style::new().paint("hello"), "hello");
    }

    #[test]
    fn fg_color() {
        assert_eq!(Style::new().fg(Color::Red).paint("err"), "\x1b[31merr\x1b[0m");
    }

    #[test]
    fn bg_color() {
        assert_eq!(Style::new().bg(Color::Blue).paint("bg"), "\x1b[44mbg\x1b[0m");
    }

    #[test]
    fn bold_fg() {
        assert_eq!(Style::new().fg(Color::Green).bold().paint("ok"), "\x1b[1;32mok\x1b[0m");
    }

    #[test]
    fn dim_text() {
        assert_eq!(Style::new().dim().paint("faint"), "\x1b[2mfaint\x1b[0m");
    }

    #[test]
    fn italic_text() {
        assert_eq!(Style::new().italic().paint("slant"), "\x1b[3mslant\x1b[0m");
    }

    #[test]
    fn underline_text() {
        assert_eq!(Style::new().underline().paint("uline"), "\x1b[4muline\x1b[0m");
    }

    #[test]
    fn fg_and_bg() {
        assert_eq!(Style::new().fg(Color::White).bg(Color::Red).paint("alert"), "\x1b[37;41malert\x1b[0m");
    }

    #[test]
    fn fixed_256_color() {
        assert_eq!(Style::new().fg(Color::Fixed(200)).paint("pink"), "\x1b[38;5;200mpink\x1b[0m");
    }

    #[test]
    fn rgb_color() {
        assert_eq!(Style::new().fg(Color::Rgb(255, 100, 0)).paint("orange"), "\x1b[38;2;255;100;0morange\x1b[0m");
    }

    #[test]
    fn fixed_256_bg() {
        assert_eq!(Style::new().bg(Color::Fixed(42)).paint("x"), "\x1b[48;5;42mx\x1b[0m");
    }

    #[test]
    fn rgb_bg() {
        assert_eq!(Style::new().bg(Color::Rgb(10, 20, 30)).paint("x"), "\x1b[48;2;10;20;30mx\x1b[0m");
    }

    #[test]
    fn all_attributes() {
        assert_eq!(
            Style::new().bold().dim().italic().underline().fg(Color::Cyan).paint("x"),
            "\x1b[1;2;3;4;36mx\x1b[0m"
        );
    }
}
