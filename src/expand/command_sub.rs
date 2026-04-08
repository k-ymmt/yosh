use crate::env::ShellEnv;
use crate::parser::ast::Program;

/// Execute a command substitution and return its output.
/// Phase 3 stub: always returns empty string.
pub fn execute(_env: &mut ShellEnv, _program: &Program) -> String {
    String::new()
}
