use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub struct ShellError {
    pub kind: ShellErrorKind,
    pub message: String,
    pub location: Option<SourceLocation>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SourceLocation {
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ShellErrorKind {
    Parse(ParseErrorKind),
    Expansion(ExpansionErrorKind),
    Runtime(RuntimeErrorKind),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ParseErrorKind {
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

#[derive(Debug, Clone, Copy, PartialEq)]
#[allow(dead_code)]
pub enum ExpansionErrorKind {
    DivisionByZero,
    UnsetVariable,
    ParameterError,
    InvalidArithmetic,
}

#[derive(Debug, Clone, Copy, PartialEq)]
#[allow(dead_code)]
pub enum RuntimeErrorKind {
    CommandNotFound,
    PermissionDenied,
    RedirectFailed,
    ReadonlyVariable,
    InvalidOption,
}

impl fmt::Display for ShellError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.location {
            Some(loc) => write!(f, "kish: line {}: {}", loc.line, self.message),
            None => write!(f, "kish: {}", self.message),
        }
    }
}

impl std::error::Error for ShellError {}

impl ShellError {
    pub fn parse(kind: ParseErrorKind, line: usize, column: usize, message: impl Into<String>) -> Self {
        Self {
            kind: ShellErrorKind::Parse(kind),
            message: message.into(),
            location: Some(SourceLocation { line, column }),
        }
    }

    pub fn expansion(kind: ExpansionErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind: ShellErrorKind::Expansion(kind),
            message: message.into(),
            location: None,
        }
    }

    pub fn runtime(kind: RuntimeErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind: ShellErrorKind::Runtime(kind),
            message: message.into(),
            location: None,
        }
    }
}

pub type Result<T> = std::result::Result<T, ShellError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display_with_location() {
        let err = ShellError::parse(
            ParseErrorKind::UnexpectedToken,
            5,
            10,
            "unexpected ')'",
        );
        assert_eq!(err.to_string(), "kish: line 5: unexpected ')'");
    }

    #[test]
    fn test_error_display_without_location() {
        let err = ShellError::runtime(
            RuntimeErrorKind::CommandNotFound,
            "foo: not found",
        );
        assert_eq!(err.to_string(), "kish: foo: not found");
    }
}
