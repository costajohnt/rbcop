//! rbcop: a RuboCop-compatible Ruby linter on Prism.

pub mod ast;
pub mod autocorrect;
pub mod autogen;
pub mod config;
pub mod cop;
pub mod cops;
pub mod directives;
pub mod encoding;
pub mod formatter;
pub mod helpers;
pub mod legacy_syntax;
pub mod node_kind;
pub mod prelude;
pub mod project_index;
pub mod runner;
pub mod source;
pub mod syntax;
pub mod targets;
pub mod unified_diff;
pub mod yaml;
