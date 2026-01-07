mod checker;
mod parser;

pub mod codegen_go;
pub mod codegen_rust;
pub mod codegen_typescript;
pub mod schema;

use ariadne::Color;
use schema::{Schema, Span};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    Rust,
    Go,
    Typescript,
}

impl std::str::FromStr for Language {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "rust" => Ok(Language::Rust),
            "go" => Ok(Language::Go),
            "typescript" => Ok(Language::Typescript),
            _ => Err(format!("Unknown language: {}", s)),
        }
    }
}

#[derive(Debug)]
pub struct CompileError {
    pub span: Span,
    pub message: String,
    pub labels: Vec<(Span, String, Color)>,
    pub notes: Vec<String>,
}

impl CompileError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            span: Span::from(0..0),
            message: message.into(),
            labels: Vec::new(),
            notes: Vec::new(),
        }
    }

    pub fn label(mut self, span: Span, message: impl Into<String>, color: Color) -> Self {
        // Use the first label's span as the primary error span
        if self.labels.is_empty() {
            self.span = span;
        }
        self.labels.push((span, message.into(), color));
        self
    }

    pub fn note(mut self, note: impl Into<String>) -> Self {
        self.notes.push(note.into());
        self
    }
}

/// Compile a Volex schema from source code.
///
/// This performs both parsing and semantic checking.
///
/// Returns both a partial schema (if any) and all errors encountered.
/// The partial schema can be used for LSP features like go-to-definition
/// and hover even when the file has syntax errors.
pub fn compile(src: &str, language: Language) -> (Option<Schema>, Vec<CompileError>) {
    let (schema, mut errors) = parser::parse(src);

    // If we got a schema, run semantic checks on it
    if let Some(ref schema) = schema {
        let check_errors = checker::check(schema, language);
        errors.extend(check_errors);
    }

    (schema, errors)
}
