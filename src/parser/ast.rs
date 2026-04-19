use std::rc::Rc;

/// Top-level program
#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    pub commands: Vec<CompleteCommand>,
}

/// A list of complete commands — used as the body of compound commands.
pub type CommandList = Vec<CompleteCommand>;

#[derive(Debug, Clone, PartialEq)]
pub struct CompleteCommand {
    pub items: Vec<(AndOrList, Option<SeparatorOp>)>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SeparatorOp {
    Semi,
    Amp,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AndOrList {
    pub first: Pipeline,
    pub rest: Vec<(AndOrOp, Pipeline)>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AndOrOp {
    And,
    Or,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Pipeline {
    pub negated: bool,
    pub commands: Vec<Command>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    Simple(SimpleCommand),
    Compound(CompoundCommand, Vec<Redirect>),
    FunctionDef(FunctionDef),
}

#[derive(Debug, Clone, PartialEq)]
pub struct SimpleCommand {
    pub assignments: Vec<Assignment>,
    pub words: Vec<Word>,
    pub redirects: Vec<Redirect>,
    pub line: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Assignment {
    pub name: String,
    pub value: Option<Word>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CompoundCommand {
    pub kind: CompoundCommandKind,
    pub line: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CompoundCommandKind {
    BraceGroup {
        body: CommandList,
    },
    Subshell {
        body: CommandList,
    },
    If {
        condition: CommandList,
        then_part: CommandList,
        elif_parts: Vec<(CommandList, CommandList)>,
        else_part: Option<CommandList>,
    },
    For {
        var: String,
        words: Option<Vec<Word>>,
        body: CommandList,
    },
    While {
        condition: CommandList,
        body: CommandList,
    },
    Until {
        condition: CommandList,
        body: CommandList,
    },
    Case {
        word: Word,
        items: Vec<CaseItem>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct CaseItem {
    pub patterns: Vec<Word>,
    pub body: CommandList,
    pub terminator: CaseTerminator,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CaseTerminator {
    Break,
    FallThrough,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FunctionDef {
    pub name: String,
    pub body: Rc<CompoundCommand>,
    pub redirects: Vec<Redirect>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Word {
    pub parts: Vec<WordPart>,
}

impl Word {
    #[allow(dead_code)]
    pub fn literal(s: &str) -> Self {
        Word {
            parts: vec![WordPart::Literal(s.to_string())],
        }
    }

    pub fn as_literal(&self) -> Option<&str> {
        if self.parts.len() == 1
            && let WordPart::Literal(s) = &self.parts[0]
        {
            return Some(s);
        }
        None
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum WordPart {
    Literal(String),
    SingleQuoted(String),
    DoubleQuoted(Vec<WordPart>),
    DollarSingleQuoted(String),
    Parameter(ParamExpr),
    CommandSub(Program),
    ArithSub(String),
    Tilde(Option<String>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum ParamExpr {
    Simple(String),
    Positional(usize),
    Special(SpecialParam),
    Length(String),
    Default {
        name: String,
        word: Option<Word>,
        null_check: bool,
    },
    Assign {
        name: String,
        word: Option<Word>,
        null_check: bool,
    },
    Error {
        name: String,
        word: Option<Word>,
        null_check: bool,
    },
    Alt {
        name: String,
        word: Option<Word>,
        null_check: bool,
    },
    StripShortSuffix(String, Word),
    StripLongSuffix(String, Word),
    StripShortPrefix(String, Word),
    StripLongPrefix(String, Word),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SpecialParam {
    At,
    Star,
    Hash,
    Question,
    Dash,
    Dollar,
    Bang,
    Zero,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Redirect {
    pub fd: Option<i32>,
    pub kind: RedirectKind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum RedirectKind {
    Input(Word),
    Output(Word),
    OutputClobber(Word),
    Append(Word),
    HereDoc(HereDoc),
    DupInput(Word),
    DupOutput(Word),
    ReadWrite(Word),
}

#[derive(Debug, Clone, PartialEq)]
pub struct HereDoc {
    pub body: Vec<WordPart>,
    pub strip_tabs: bool,
    pub quoted: bool, // true if delimiter was quoted (no expansion needed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_word_literal() {
        let w = Word::literal("hello");
        assert_eq!(w.as_literal(), Some("hello"));
    }

    #[test]
    fn test_word_non_literal() {
        let w = Word {
            parts: vec![
                WordPart::Literal("hello".to_string()),
                WordPart::Parameter(ParamExpr::Simple("x".to_string())),
            ],
        };
        assert_eq!(w.as_literal(), None);
    }

    #[test]
    fn test_simple_command_construction() {
        let cmd = SimpleCommand {
            assignments: vec![],
            words: vec![Word::literal("echo"), Word::literal("hello")],
            redirects: vec![],
            line: 0,
        };
        assert_eq!(cmd.words.len(), 2);
    }
}
