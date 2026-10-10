//! rbcop: a RuboCop-compatible Ruby linter on Prism.

pub mod ast;
pub mod config;
pub mod cop;
pub mod cops;
pub mod directives;
pub mod formatter;
pub mod node_kind;
pub mod prelude;
pub mod runner;
pub mod source;
pub mod targets;
