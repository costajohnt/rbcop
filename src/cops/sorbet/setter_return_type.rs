use crate::helpers::sorbet_signature::{
    as_send, block_body, index_of, outermost_range, outermost_send, return_type, signature, signatures_before, siblings,
};
use crate::prelude::*;
use ruby_prism::CallNode;

pub const NAME: &str = "Sorbet/SetterReturnType";
pub static COP: SetterReturnType = SetterReturnType;
pub struct SetterReturnType;

const MSG: &str = "Setter methods must declare a `void` return type.";

/// return_declaration: the `void`/`returns` send in the body's chain.
fn return_declaration<'pr>(node: Option<Node<'pr>>) -> Option<CallNode<'pr>> {
    let call = as_send(&node?)?;
    if matches!(call.name().as_slice(), b"void" | b"returns") { Some(call) } else { return_declaration(call.receiver()) }
}

fn check_sig(cx: &mut Cx, sig: &Node) {
    let Some(decl) = return_declaration(block_body(&sig.as_call_node().unwrap())) else { return };
    let args = send_args(&decl);
    if decl.name().as_slice() == b"void" || args.is_empty() {
        return;
    }
    let Some(sel) = decl.message_loc() else { return };
    let end = decl
        .closing_loc()
        .map(|l| l.end_offset())
        .or_else(|| args.last().map(|a| helpers::node_types::range(a).1))
        .unwrap_or(sel.end_offset());
    let s = sel.start_offset();
    cx.add_fix(s, end, MSG, |c| c.replace(s, end, "void"));
}

impl Cop for SetterReturnType {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::DEF_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let name = node.as_def_node().unwrap().name();
        let name = name.as_slice();
        if !name.ends_with(b"=") || matches!(name, b"==" | b"===" | b"!=" | b"<=" | b">=") {
            return;
        }
        let ancestors = cx.ancestors;
        let target = outermost_range(ancestors, helpers::node_types::range(node));
        // The target's parser parent: a send owning a block has the block as its parent.
        let parent = match outermost_send(ancestors) {
            None => ancestors.last(),
            Some(i) if ancestors.get(i + 1).is_some_and(|n| n.as_block_node().is_some()) => None,
            Some(i) => i.checked_sub(1).map(|p| &ancestors[p]),
        };
        // preceding_sigs: the signatures right before the target.
        let sibs = siblings(parent).unwrap_or_default();
        let i = index_of(&sibs, target).unwrap_or(0);
        let n = sibs[..i].iter().rev().take_while(|s| signature(s).is_some()).count();
        if n > 0 {
            for sig in &sibs[i - n..i] {
                check_sig(cx, sig);
            }
            return;
        }
        // check_rbs
        for group in signatures_before(cx, target) {
            let Some(rt) = return_type(cx.src, &group) else { continue };
            if rt.void {
                continue;
            }
            let ((hs, he), (rs, re)) = (rt.highlight, rt.replace);
            cx.add_fix(hs, he, MSG, |c| c.replace(rs, re, "void"));
        }
    }
}
