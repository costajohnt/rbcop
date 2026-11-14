use crate::helpers::parser_parent::parser_parent;
use crate::helpers::sorbet_rbs_assertion::{assertion_statement, autocorrectable, is_t_call, parser_range, send_parent};
use crate::prelude::*;

pub const NAME: &str = "Sorbet/ForbidTAbsurd";
pub static COP: ForbidTAbsurd = ForbidTAbsurd;
pub struct ForbidTAbsurd;

/// `inline_else_statement`: the index of the `if`/`unless`/`case` whose `else` branch is just
/// this node, with `else` on the node's line.
fn inline_else(src: &crate::source::Source, ancestors: &[Node], start: usize) -> Option<usize> {
    let n = ancestors.len();
    let stmts = ancestors.get(n.checked_sub(1)?)?.as_statements_node()?;
    if stmts.body().iter().count() != 1 {
        return None;
    }
    let els = ancestors.get(n.checked_sub(2)?)?.as_else_node()?;
    let kw = els.else_keyword_loc();
    let i = n.checked_sub(3)?;
    if !matches!(ancestors[i], Node::IfNode { .. } | Node::UnlessNode { .. } | Node::CaseNode { .. }) {
        return None;
    }
    (kw.as_slice() == b"else" && src.line_of(kw.start_offset()) == src.line_of(start)).then_some(i)
}

impl Cop for ForbidTAbsurd {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (send (const nil? :T) :absurd _)
        let call = node.as_call_node().unwrap();
        if !is_t_call(&call, "absurd", Some(1)) {
            return;
        }
        let (s, e) = send_range(&call);
        let statement = match inline_else(cx.src, cx.ancestors, s).filter(|_| !call.block().is_some_and(|b| b.as_block_node().is_some())) {
            Some(i) => assertion_statement(parser_parent(&cx.ancestors[..i]), parser_range(&cx.ancestors[i]), true),
            None => assertion_statement(send_parent(&call, cx.ancestors), (s, e), true),
        };
        let fix = autocorrectable(cx, (s, e), statement);
        let replacement = format!("{} #: absurd", node_text(&send_args(&call)[0]));
        cx.add_fix(s, e, "Do not use `T.absurd`.", |c| {
            if fix {
                c.replace(s, e, &replacement);
            }
        });
    }
}
