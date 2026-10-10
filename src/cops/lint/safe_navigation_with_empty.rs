use crate::prelude::*;

pub const NAME: &str = "Lint/SafeNavigationWithEmpty";
pub static COP: SafeNavigationWithEmpty = SafeNavigationWithEmpty;
pub struct SafeNavigationWithEmpty;

impl Cop for SafeNavigationWithEmpty {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::IF_NODE, kind::UNLESS_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        if is_pattern_guard(node, cx.ancestors) {
            return;
        }
        let cond = match node.as_if_node() {
            Some(i) => i.predicate(),
            None => node.as_unless_node().unwrap().predicate(),
        };
        // (csend !csend :empty?): no arguments, no block, receiver not a csend.
        let Some(call) = cond.as_call_node() else { return };
        if !is_csend(&call) || method_name(&call) != "empty?" || call.arguments().is_some() || call.block().is_some() {
            return;
        }
        let Some(recv) = call.receiver() else { return };
        let recv_csend = recv
            .as_call_node()
            .is_some_and(|r| is_csend(&r) && !r.block().is_some_and(|b| b.as_block_node().is_some()));
        if recv_csend {
            return;
        }
        let l = cond.location();
        let (s, e) = (l.start_offset(), l.end_offset());
        let r = node_text(&recv);
        let fix = format!("{r} && {r}.empty?");
        cx.add_fix(s, e, "Avoid calling `empty?` with the safe navigation operator in conditionals.", |c| {
            c.replace(s, e, &fix)
        });
    }
}
