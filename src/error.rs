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
    #[allow(dead_code)] // matched in parse_status; will be constructed by parser enhancements
    UnexpectedEof,
    InvalidRedirect,
    #[allow(dead_code)] // will be constructed by function definition parsing
    InvalidFunctionName,
    InvalidHereDoc,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ExpansionErrorKind {
    DivisionByZero,
    InvalidArithmetic,
    /// Unterminated `${`, `$(`, `$((`, or `` ` `` found while expanding a
    /// raw string (heredoc body / arithmetic text) at end of input.
    UnterminatedExpansion,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RuntimeErrorKind {
    CommandNotFound,
    #[allow(dead_code)] // kept for exit-code mapping symmetry (126)
    PermissionDenied,
    RedirectFailed,
    ReadonlyVariable,
    InvalidOption,
    InvalidArgument,
    IoError,
    #[allow(dead_code)] // kept for exit-code mapping symmetry (126)
    ExecFailed,
    #[allow(dead_code)] // reserved for future trap-handler error reporting
    TrapError,
    JobControlError,
}

impl fmt::Display for ShellError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.location {
            Some(loc) => write!(f, "yosh: line {}: {}", loc.line, self.message),
            None => write!(f, "yosh: {}", self.message),
        }
    }
}

impl std::error::Error for ShellError {}

impl ShellError {
    pub fn parse(
        kind: ParseErrorKind,
        line: usize,
        column: usize,
        message: impl Into<String>,
    ) -> Self {
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

    /// True when the POSIX §2.8.1 consequences table requires a
    /// non-interactive shell to exit on this error: expansion errors
    /// and variable-assignment errors.
    pub fn requires_noninteractive_exit(&self) -> bool {
        matches!(
            self.kind,
            ShellErrorKind::Expansion(_)
                | ShellErrorKind::Runtime(RuntimeErrorKind::ReadonlyVariable)
        )
    }

    /// Map this error to an appropriate POSIX exit code.
    pub fn exit_code(&self) -> i32 {
        match &self.kind {
            ShellErrorKind::Parse(_) => 2,
            ShellErrorKind::Expansion(_) => 1,
            ShellErrorKind::Runtime(r) => match r {
                RuntimeErrorKind::CommandNotFound => 127,
                RuntimeErrorKind::PermissionDenied | RuntimeErrorKind::ExecFailed => 126,
                RuntimeErrorKind::InvalidOption | RuntimeErrorKind::InvalidArgument => 2,
                _ => 1,
            },
        }
    }
}

pub type Result<T> = std::result::Result<T, ShellError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display_with_location() {
        let err = ShellError::parse(ParseErrorKind::UnexpectedToken, 5, 10, "unexpected ')'");
        assert_eq!(err.to_string(), "yosh: line 5: unexpected ')'");
    }

    #[test]
    fn test_error_display_without_location() {
        let err = ShellError::runtime(RuntimeErrorKind::CommandNotFound, "foo: not found");
        assert_eq!(err.to_string(), "yosh: foo: not found");
    }

    #[test]
    fn test_runtime_error_new_variants() {
        let err = ShellError::runtime(RuntimeErrorKind::InvalidArgument, "bad arg");
        assert_eq!(err.to_string(), "yosh: bad arg");

        let err = ShellError::runtime(RuntimeErrorKind::IoError, "read failed");
        assert_eq!(err.to_string(), "yosh: read failed");
    }

    #[test]
    fn test_exit_code_mapping() {
        assert_eq!(
            ShellError::runtime(RuntimeErrorKind::CommandNotFound, "x").exit_code(),
            127
        );
        assert_eq!(
            ShellError::runtime(RuntimeErrorKind::PermissionDenied, "x").exit_code(),
            126
        );
        assert_eq!(
            ShellError::runtime(RuntimeErrorKind::InvalidArgument, "x").exit_code(),
            2
        );
        assert_eq!(
            ShellError::runtime(RuntimeErrorKind::IoError, "x").exit_code(),
            1
        );
        assert_eq!(
            ShellError::runtime(RuntimeErrorKind::RedirectFailed, "x").exit_code(),
            1
        );
    }
}
