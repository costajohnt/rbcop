use crate::prelude::*;
use helpers::minitest::*;

pub const NAME: &str = "Minitest/ReturnInTestMethod";
pub static COP: ReturnInTestMethod = ReturnInTestMethod;
pub struct ReturnInTestMethod;

impl Cop for ReturnInTestMethod {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::RETURN_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let anc = cx.ancestors;
        if anc.iter().any(|a| matches!(a, Node::BlockNode { .. } | Node::LambdaNode { .. })) {
            return;
        }
        if !(0..anc.len()).any(|i| test_case(&anc[i], &anc[..i])) {
            return;
        }
        let (s, e) = range(node);
        cx.add_fix(s, e, "Use `skip` instead of `return`.", |c| c.replace(s, e, "skip"));
    }
}
