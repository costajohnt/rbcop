use crate::prelude::*;
use ruby_prism::Visit;

pub const NAME: &str = "Style/UnlessLogicalOperators";
pub static COP: UnlessLogicalOperators = UnlessLogicalOperators;
pub struct UnlessLogicalOperators;

/// Operators of every `and` / `or` node in a subtree (the root included).
#[derive(Default)]
struct Ops {
    and: Vec<Vec<u8>>,
    or: Vec<Vec<u8>>,
}

impl<'pr> Visit<'pr> for Ops {
    fn visit_and_node(&mut self, node: &ruby_prism::AndNode<'pr>) {
        self.and.push(node.operator_loc().as_slice().to_vec());
        ruby_prism::visit_and_node(self, node);
    }
    fn visit_or_node(&mut self, node: &ruby_prism::OrNode<'pr>) {
        self.or.push(node.operator_loc().as_slice().to_vec());
        ruby_prism::visit_or_node(self, node);
    }
}

fn mixed(ops: &[Vec<u8>], symbolic: &[u8], word: &[u8]) -> bool {
    !(ops.iter().all(|o| o == symbolic) || ops.iter().all(|o| o == word))
}

impl Cop for UnlessLogicalOperators {
    fn kinds(&self) -> &'static [usize] {
        &[kind::UNLESS_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        if is_pattern_guard(node, cx.ancestors) {
            return;
        }
        let cond = node.as_unless_node().unwrap().predicate();
        let is_and = cond.as_and_node().is_some();
        let is_or = cond.as_or_node().is_some();
        let msg = if cx.cfg.str("EnforcedStyle") == Some("forbid_logical_operators") {
            if !(is_and || is_or) {
                return;
            }
            "Do not use any logical operator in an `unless`."
        } else {
            let mut ops = Ops::default();
            ops.visit(&cond);
            let offense = (is_or && !ops.and.is_empty())
                || (is_and && !ops.or.is_empty())
                || mixed(&ops.and, b"&&", b"and")
                || mixed(&ops.or, b"||", b"or");
            if !offense {
                return;
            }
            "Do not use mixed logical operators in an `unless`."
        };
        let loc = node.location();
        cx.add(loc.start_offset(), loc.end_offset(), msg);
    }
}
