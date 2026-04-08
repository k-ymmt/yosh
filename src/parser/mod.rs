pub mod ast;

use crate::error;
use ast::Program;

pub struct Parser {
    input: String,
}

impl Parser {
    pub fn new(input: &str) -> Self {
        Self {
            input: input.to_string(),
        }
    }

    pub fn parse_program(&mut self) -> error::Result<Program> {
        // Stub — returns empty program
        let _ = &self.input;
        Ok(Program { commands: vec![] })
    }
}
