use std::io;

use crossterm::style::Color;

use super::terminal::Terminal;

// Re-export types from split modules so downstream `use super::highlight::...` continues to work.
pub use super::command_checker::CheckerEnv;
#[allow(unused_imports)]
pub use super::command_checker::{CommandChecker, CommandExistence};
pub use super::highlight_scanner::HighlightScanner;

// ---------------------------------------------------------------------------
// HighlightStyle
// ---------------------------------------------------------------------------

/// Visual style applied to a span of characters in the input line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HighlightStyle {
    Default,
    Keyword,
    Operator,
    Redirect,
    String,
    DoubleString,
    Variable,
    CommandSub,
    ArithSub,
    Comment,
    CommandValid,
    CommandInvalid,
    IoNumber,
    Assignment,
    Tilde,
    Error,
}

// ---------------------------------------------------------------------------
// ColorSpan
// ---------------------------------------------------------------------------

/// A half-open byte range [start, end) with an associated style.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColorSpan {
    pub start: usize,
    pub end: usize,
    pub style: HighlightStyle,
}

// ---------------------------------------------------------------------------
// apply_style
// ---------------------------------------------------------------------------

/// Apply the terminal attributes associated with `style`.
pub fn apply_style<T: Terminal>(term: &mut T, style: HighlightStyle) -> io::Result<()> {
    match style {
        HighlightStyle::Default => {
            // No styling needed.
        }
        HighlightStyle::Keyword => {
            term.set_bold(true)?;
            term.set_fg_color(Color::Magenta)?;
        }
        HighlightStyle::Operator | HighlightStyle::Redirect => {
            term.set_fg_color(Color::Cyan)?;
        }
        HighlightStyle::String | HighlightStyle::DoubleString => {
            term.set_fg_color(Color::Yellow)?;
        }
        HighlightStyle::Variable | HighlightStyle::Tilde => {
            term.set_bold(true)?;
            term.set_fg_color(Color::Green)?;
        }
        HighlightStyle::CommandSub | HighlightStyle::ArithSub => {
            term.set_bold(true)?;
            term.set_fg_color(Color::Yellow)?;
        }
        HighlightStyle::Comment => {
            term.set_dim(true)?;
        }
        HighlightStyle::CommandValid => {
            term.set_bold(true)?;
            term.set_fg_color(Color::Green)?;
        }
        HighlightStyle::CommandInvalid => {
            term.set_bold(true)?;
            term.set_fg_color(Color::Red)?;
        }
        HighlightStyle::IoNumber | HighlightStyle::Assignment => {
            term.set_fg_color(Color::Blue)?;
        }
        HighlightStyle::Error => {
            term.set_fg_color(Color::Red)?;
            term.set_underline(true)?;
        }
    }
    Ok(())
}
