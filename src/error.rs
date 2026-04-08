use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub struct ShellError {
    pub kind: ShellErrorKind,
    pub line: usize,
    pub column: usize,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ShellErrorKind {
    UnterminatedSingleQuote,
    UnterminatedDoubleQuote,
    UnterminatedCommandSub,
    UnterminatedArithSub,
    UnterminatedParamExpansion,
    UnterminatedBacktick,
    UnterminatedDollarSingleQuote,
    UnexpectedToken,
    UnexpectedEof,
    InvalidRedirect,
    InvalidFunctionName,
    InvalidHereDoc,
}

impl fmt::Display for ShellError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "kish: line {}: {}", self.line, self.message)
    }
}

impl std::error::Error for ShellError {}

impl ShellError {
    pub fn new(kind: ShellErrorKind, line: usize, column: usize, message: impl Into<String>) -> Self {
        Self { kind, line, column, message: message.into() }
    }
}

pub type Result<T> = std::result::Result<T, ShellError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = ShellError::new(
            ShellErrorKind::UnexpectedToken,
            5,
            10,
            "unexpected ')'",
        );
        assert_eq!(err.to_string(), "kish: line 5: unexpected ')'");
    }
}
