//! Shared compiler and source-container implementation used by Studio and CLI.
#![forbid(unsafe_code)]

pub mod codec;
mod codegen;
mod lexer;
mod parser;
mod semantics;
pub mod storage;
mod strings;

pub use lexer::{Highlight, HighlightKind, highlight};
pub use parser::{FunctionInfo, analyze};
use std::{fmt, ops::Range};

pub const MAX_SOURCE: usize = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub message: String,
    pub span: Range<usize>,
    pub line: usize,
    pub column: usize,
}

impl Diagnostic {
    pub(crate) fn new(source: &str, span: Range<usize>, message: impl Into<String>) -> Self {
        let start = span.start.min(source.len());
        let prefix = source.get(..start).unwrap_or("");
        Self {
            message: message.into(),
            span,
            line: prefix.bytes().filter(|&b| b == b'\n').count() + 1,
            column: prefix.rsplit('\n').next().unwrap_or("").chars().count() + 1,
        }
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ERROR {}:{}: {}", self.line, self.column, self.message)
    }
}
impl std::error::Error for Diagnostic {}

#[derive(Debug)]
pub struct Build {
    pub executable: Vec<u8>,
    pub functions: Vec<FunctionInfo>,
    pub code_bytes: usize,
}

/// Compiles directly to a standalone Windows x64 PE executable.
pub fn compile(source: &str) -> Result<Build, Diagnostic> {
    if source.len() > MAX_SOURCE {
        return Err(Diagnostic::new(source, 0..0, "source exceeds 1 MiB"));
    }
    let tokens = lexer::lex(source)?;
    let program = parser::parse(source, tokens)?;
    let functions = program.functions.iter().map(FunctionInfo::from).collect();
    let (executable, code_bytes) = codegen::generate(source, &program)?;
    Ok(Build {
        executable,
        functions,
        code_bytes,
    })
}
