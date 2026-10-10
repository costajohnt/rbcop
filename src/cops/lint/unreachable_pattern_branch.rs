use crate::prelude::*;

pub const NAME: &str = "Lint/UnreachablePatternBranch";
pub static COP: UnreachablePatternBranch = UnreachablePatternBranch;
pub struct UnreachablePatternBranch;

/// `catch_all_pattern?`: a bare variable, possibly captured (`=> x`), parenthesised or one of
/// the alternatives.
fn catch_all(pattern: &Node) -> bool {
    match kind::of(pattern) {
        kind::LOCAL_VARIABLE_TARGET_NODE => true,
        kind::CAPTURE_PATTERN_NODE => catch_all(&pattern.as_capture_pattern_node().unwrap().value()),
        kind::PARENTHESES_NODE => pattern.as_parentheses_node().unwrap().body().is_some_and(|b| match b.as_statements_node() {
            Some(s) => s.body().iter().next().is_some_and(|f| catch_all(&f)),
            None => catch_all(&b),
        }),
        kind::ALTERNATION_PATTERN_NODE => {
            let alt = pattern.as_alternation_pattern_node().unwrap();
            catch_all(&alt.left()) || catch_all(&alt.right())
        }
        _ => false,
    }
}

impl Cop for UnreachablePatternBranch {
    fn ruby_range(&self) -> (f64, f64) {
        (2.7, f64::MAX)
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CASE_MATCH_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let case = node.as_case_match_node().unwrap();
        let mut found = false;
        for b in case.conditions().iter() {
            let branch = b.as_in_node().unwrap();
            let pattern = branch.pattern();
            if found {
                let start = b.location().start_offset();
                // Parser's `in` ends at its body, or at its pattern (guard included) without one.
                let end = branch.statements().map_or(pattern.location().end_offset(), |s| s.location().end_offset());
                cx.add(start, end, "Unreachable `in` pattern branch detected.");
                continue;
            }
            // A guard (`in x if c`) is a Prism IfNode/UnlessNode around the pattern.
            found = catch_all(&pattern);
        }
        if found && let Some(e) = case.else_clause() {
            let kw = e.else_keyword_loc();
            cx.add(kw.start_offset(), kw.end_offset(), "Unreachable `else` branch detected.");
        }
    }
}
