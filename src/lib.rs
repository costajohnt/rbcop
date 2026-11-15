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

/// `--no-ruby` / `RBCOP_NO_RUBY=1` (#771): rbcop starts no `ruby`, `rubocop` or `bundle`. What
/// needs Ruby fails with `refuse`'s error; the RuboCop fallback is skipped and reported instead
/// (src/fallback.rs). Either wins over RBCOP_FALLBACK, `=1` included.
pub mod no_ruby {
    use std::sync::atomic::{AtomicBool, Ordering::Relaxed};

    static FLAG: AtomicBool = AtomicBool::new(false);
    const PREFIX: &str = "rbcop: --no-ruby: ";

    /// `--no-ruby` was given.
    pub fn set() {
        FLAG.store(true, Relaxed);
    }

    pub fn on() -> bool {
        FLAG.load(Relaxed) || std::env::var_os("RBCOP_NO_RUBY").is_some_and(|v| v == "1")
    }

    /// Err when Ruby is off, for what would start it (`what` names it).
    pub fn refuse(what: &str) -> Result<(), String> {
        match on() {
            true => Err(format!("{PREFIX}{what} needs Ruby, which --no-ruby (or RBCOP_NO_RUBY=1) forbids")),
            false => Ok(()),
        }
    }

    /// `e` is `refuse`'s error.
    pub fn refused(e: &str) -> bool {
        e.starts_with(PREFIX)
    }
}
