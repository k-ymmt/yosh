//! Nesting limits shared by the parser and the lexer.
//!
//! Parsing is recursive descent and the lexer recurses into nested word
//! constructs (and into a fresh parser for `$(...)` / backquote bodies),
//! so unbounded nesting would overflow the native stack and abort the
//! shell. Three independent checks turn that into a syntax error:
//!
//! - `MAX_NESTING_DEPTH`: syntactic nesting (compound commands and nested
//!   programs), counted per thread by `NestingGuard`.
//! - `MAX_NEST_DEPTH`: word-level nesting (`${...}` words, quotes,
//!   `$(...)` bodies), counted per lexer (`Lexer::check_nest_depth`) and
//!   inherited by the sub-lexers of nested bodies.
//! - `PARSE_STACK_BUDGET`: the actual stack depth, checked by every level
//!   of both kinds through `StackBudget`, which also checks the absolute
//!   stack left (`exec::stack_guard::descent_stack_exhausted`) for a
//!   parse that began deep in the stack.

use crate::error::{self, ParseErrorKind, ShellError};
use crate::lexer::token::Span;

/// Maximum syntactic nesting depth: compound commands (`(`, `{`, `if`,
/// `while`, `for`, `case`, function bodies, ...) plus nested programs
/// (`$(...)` / backquote bodies, which the lexer parses with a fresh
/// parser on the same native stack). Parsing is recursive descent, so
/// unbounded nesting would overflow the stack and abort the shell; past
/// this depth the parser reports a syntax error instead. The limit is
/// chosen to stay well inside the 8 MiB main-thread stack in debug builds.
pub(crate) const MAX_NESTING_DEPTH: usize = 200;

/// Maximum nesting depth of word-level constructs before the lexer gives
/// up with a syntax error instead of overflowing the native stack.
///
/// Each `read_word_parts` level (a `${x-word}` word, a double-quoted
/// string, a word inside `$(...)`) costs 1; entering a nested command
/// substitution body costs `SUBPARSE_NEST_COST` more, since it also
/// recurses through the parser. Measured on a debug build (8 MiB main
/// stack): ~11 KiB of stack per unit for `${x:-${x:-...}}` (overflow at
/// ~750 levels) and ~32 KiB per `$(echo $(echo ...))` level (overflow at
/// ~250). 400 units keeps the worst case near 4.4 MiB in debug builds
/// (far less in release), leaving room for the executor frames below a
/// nested parse (`eval`, `.`, functions). Real scripts nest a handful of
/// levels.
pub(crate) const MAX_NEST_DEPTH: usize = 400;

/// Extra nesting cost of a command substitution body (see
/// `MAX_NEST_DEPTH`).
pub(crate) const SUBPARSE_NEST_COST: usize = 4;

/// Native stack a single parse may use, measured from where the
/// outermost active nesting guard on this thread was entered.
///
/// `MAX_NESTING_DEPTH` (compound commands) and the lexer's
/// `MAX_NEST_DEPTH` (word-level `${...}` / quote / `$(...)` nesting) are
/// independent counters, and each alone fits the stack, but their frames
/// add up: measured on a debug build, a compound level costs up to
/// ~21 KiB and a `${x:-` word level ~14 KiB, so 190 `if`s around 350
/// nested `${x:-...}` (both under their own limits) overflowed the 8 MiB
/// main-thread stack. Both kinds of nesting therefore also check the
/// actual stack depth against this one shared budget. 6 MiB leaves ~2 MiB
/// for the frames below the outermost parse (`main`, and the executor
/// frames under an `eval` / `.` / command-substitution parse).
pub(crate) const PARSE_STACK_BUDGET: usize = 6 << 20;

thread_local! {
    /// Nesting depth summed over every parser active on this thread, so
    /// that a command substitution inside deeply nested compounds counts
    /// toward the same limit as the compounds around it.
    static NESTING: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

thread_local! {
    /// `(floor, active)`: the lowest stack pointer at which a
    /// `StackBudget` guard may still be entered, fixed when the outermost
    /// guard on this thread was entered, and the number of guards
    /// currently alive.
    static PARSE_STACK: std::cell::Cell<(usize, usize)> = const { std::cell::Cell::new((0, 0)) };
}

/// The "nesting too deep" syntax error every limit above reports.
fn too_deep(line: usize, column: usize, message: impl Into<String>) -> ShellError {
    ShellError::parse(ParseErrorKind::UnexpectedToken, line, column, message)
}

/// Approximate current stack pointer: the address of a local in a frame
/// that is never inlined. Good to within one frame, which is all the
/// budget check needs. Assumes a downward-growing stack (every target yosh
/// supports); on an upward-growing stack the check never fires.
#[inline(never)]
fn approx_stack_pointer() -> usize {
    let probe = 0u8;
    std::hint::black_box(&probe) as *const u8 as usize
}

/// RAII token for one recursive parse/lex level counted against
/// `PARSE_STACK_BUDGET`.
pub(crate) struct StackBudget;

impl StackBudget {
    pub(crate) fn enter(line: usize, column: usize) -> error::Result<Self> {
        let sp = approx_stack_pointer();
        PARSE_STACK.with(|c| {
            let (mut floor, active) = c.get();
            if active == 0 {
                // The budget is relative to where the outermost parse
                // began (`base - sp > PARSE_STACK_BUDGET` is too deep); a
                // parse begun deep in shell-level recursion (`eval` inside
                // a recursive function) can run out of actual stack
                // first (`stack_guard::descent_stack_exhausted`). Both
                // limits are fixed while the outermost guard is alive, so
                // fold them into one floor for the whole parse.
                floor = crate::exec::stack_guard::descent_floor()
                    .max(sp.saturating_sub(PARSE_STACK_BUDGET));
            }
            if sp < floor {
                return Err(too_deep(line, column, "syntax error: nesting too deep"));
            }
            c.set((floor, active + 1));
            Ok(StackBudget)
        })
    }
}

impl Drop for StackBudget {
    fn drop(&mut self) {
        PARSE_STACK.with(|c| {
            let (floor, active) = c.get();
            c.set((floor, active - 1));
        });
    }
}

/// Check one word-level nesting level at `depth` (see `MAX_NEST_DEPTH`
/// and `PARSE_STACK_BUDGET`). The returned guard keeps this level counted
/// against the shared stack budget while it is alive.
#[inline]
pub(crate) fn enter_word_level(
    depth: usize,
    line: usize,
    column: usize,
) -> error::Result<StackBudget> {
    if depth > MAX_NEST_DEPTH {
        return Err(too_deep(line, column, "syntax error: nesting too deep"));
    }
    StackBudget::enter(line, column)
}

/// RAII token for one level of syntactic nesting (see `MAX_NESTING_DEPTH`
/// and `PARSE_STACK_BUDGET`).
pub(crate) struct NestingGuard {
    _stack: StackBudget,
}

impl NestingGuard {
    pub(crate) fn enter(span: Span) -> error::Result<Self> {
        let stack = StackBudget::enter(span.line, span.column)?;
        NESTING.with(|n| {
            if n.get() >= MAX_NESTING_DEPTH {
                return Err(too_deep(
                    span.line,
                    span.column,
                    format!("syntax error: nesting too deep (limit {MAX_NESTING_DEPTH})"),
                ));
            }
            n.set(n.get() + 1);
            Ok(NestingGuard { _stack: stack })
        })
    }
}

impl Drop for NestingGuard {
    fn drop(&mut self) {
        NESTING.with(|n| n.set(n.get() - 1));
    }
}
