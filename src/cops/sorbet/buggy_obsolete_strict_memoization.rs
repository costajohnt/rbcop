use crate::cops::sorbet_obsolete_strict_memoization::{begin_statements, legacy};
use crate::helpers::sorbet_target_version::sorbet_enabled;
use crate::prelude::*;

pub const NAME: &str = "Sorbet/BuggyObsoleteStrictMemoization";
pub static COP: BuggyObsoleteStrictMemoization = BuggyObsoleteStrictMemoization;
pub struct BuggyObsoleteStrictMemoization;

const MSG: &str = "This might be a mistaken variant of the two-stage workaround that used to be needed for memoization \
in `#typed: strict` files. See https://sorbet.org/docs/type-assertions#put-type-assertions-behind-memoization.";

impl Cop for BuggyObsoleteStrictMemoization {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::STATEMENTS_NODE]
    }

    fn on_file(&self, cx: &mut Cx) {
        // relevant_file? raises without a lockfile next to the Gemfile (RuboCop aborts the run).
        if sorbet_enabled().is_err() {
            crate::runner::cop_error_at(NAME, cx.src, None);
        }
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        if sorbet_enabled() != Ok(true) {
            return;
        }
        let Some(stmts) = begin_statements(node, cx.ancestors) else { return };
        let Some(l) = legacy(&stmts) else { return };
        let Some((s, e)) = l.let_nil else { return };
        let ivar = l.ivar;
        cx.add_fix(s, e, MSG, |c| c.replace(s, e, ivar));
    }
}
