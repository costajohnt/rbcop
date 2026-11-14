use crate::helpers::sorbet_signature::{block_body, each_call, index_of, signature, siblings};
use crate::prelude::*;
use ruby_prism::CallNode;

pub const NAME: &str = "Sorbet/VoidCheckedTests";
pub static COP: VoidCheckedTests = VoidCheckedTests;
pub struct VoidCheckedTests;

const MESSAGE: &str = "Returning `.void` from a sig marked `.checked(:tests)` means that the method will return a different value in non-test environments (possibly with different truthiness). Either use `.returns(T.anything).checked(:tests)` to keep checking in tests, or `.void.checked(:never)` to leave it untouched.";

/// top_level_void: the `void` call in the chain of SendNodes (`.` or `&.`, no literal block).
fn top_level_void<'pr>(node: Option<Node<'pr>>) -> Option<CallNode<'pr>> {
    let call = node?.as_call_node().filter(|c| c.block().is_none_or(|b| b.as_block_node().is_none()))?;
    if call.name().as_slice() == b"void" { Some(call) } else { top_level_void(call.receiver()) }
}

impl Cop for VoidCheckedTests {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        if signature(node).is_none() {
            return;
        }
        let mut checked = false;
        each_call(node, |c| {
            let tests = |a: &Node| a.as_symbol_node().is_some_and(|s| s.unescaped() == b"tests");
            checked |= c.name().as_slice() == b"checked" && matches!(send_args(c).as_slice(), [a] if tests(a));
        });
        if !checked {
            return;
        }
        if let Some(sibs) = siblings(cx.ancestors.last())
            && let Some(i) = index_of(&sibs, helpers::node_types::range(node))
            && let Some(def) = sibs[i + 1..].iter().find_map(|s| s.as_def_node())
            && def.name().as_slice() == b"initialize"
        {
            return;
        }
        let call = node.as_call_node().unwrap();
        let Some(void) = top_level_void(block_body(&call)) else { return };
        let Some(sel) = void.message_loc() else { return };
        let (s, e) = (sel.start_offset(), sel.end_offset());
        cx.add_fix(s, e, MESSAGE, |c| c.replace(s, e, "returns(T.anything)"));
    }
}
