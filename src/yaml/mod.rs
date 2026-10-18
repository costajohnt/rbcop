//! libyaml 0.2.5 (the C library under Psych) ported to Rust: its reader, scanner and parser
//! (`src/reader.c`, `scanner.c`, `parser.c`), so `.rubocop.yml` loads with Psych's tokens, events,
//! errors and marks, and its emitter (`emitter.c`), so `--show-cops` writes values as Psych does.
//! Function names follow libyaml's (`yaml_parser_scan_plain_scalar` is `scan_plain_scalar`).

pub mod emitter;
pub mod parser;
pub mod scanner;

/// A position: characters from the start (a CRLF counts 2), 0-based line and column.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Mark {
    pub index: usize,
    pub line: usize,
    pub column: usize,
}

/// A reader, scanner or parser error, as libyaml records it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    pub problem: &'static str,
    pub context: Option<&'static str>,
    /// Left at the start (zero) by errors without a context (reader errors, `set_parser_error`).
    pub context_mark: Mark,
}

impl Error {
    /// Psych::SyntaxError#message: the problem, the context, and the context mark (1-based).
    pub fn message(&self, path: &str) -> String {
        let what = match self.context {
            Some(c) => format!("{} {c}", self.problem),
            None => self.problem.to_string(),
        };
        format!("({path}): {what} at line {} column {}", self.context_mark.line + 1, self.context_mark.column + 1)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScalarStyle {
    /// Emitter input only: let the emitter choose.
    Any,
    Plain,
    SingleQuoted,
    DoubleQuoted,
    Literal,
    Folded,
}
