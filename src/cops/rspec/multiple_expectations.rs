use crate::prelude::*;
use ruby_prism::{CallNode, Visit};

pub const NAME: &str = "RSpec/MultipleExpectations";
pub static COP: MultipleExpectations = MultipleExpectations;
pub struct MultipleExpectations;

/// aggregate_failures?(block, matcher): `(block {(send _ _ <(sym :aggregate_failures) ...>)
/// (send _ _ ... (hash <(pair (sym :aggregate_failures) %1) ...>))} ...)`; `true_only` for TRUE_NODE.
fn aggregate_failures(cx: &Cx, call: &CallNode, true_only: bool) -> bool {
    if is_csend(call) {
        return false;
    }
    let args = send_args(call);
    let is_key = |n: &Node| helpers::rspec::is_sym(cx, n) && helpers::rails::sym_value(n).as_deref() == Some("aggregate_failures");
    if args.iter().any(&is_key) {
        return true;
    }
    let Some(pairs) = args.last().and_then(helpers::rails::hash_elements) else { return false };
    pairs.iter().filter_map(|p| p.as_assoc_node()).any(|p| is_key(&p.key()) && (!true_only || p.value().as_true_node().is_some()))
}

impl Cop for MultipleExpectations {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let l = helpers::rspec::lang(cx);
        if !helpers::rspec::example(&l, node) {
            return;
        }
        let call = node.as_call_node().unwrap();
        // find_aggregate_failures: the example itself, then the plain blocks around it
        let found = std::iter::once(call.as_node().as_call_node().unwrap())
            .chain(cx.ancestors.iter().rev().filter_map(helpers::rspec::plain_block_call))
            .find(|c| aggregate_failures(cx, c, false));
        if found.is_some_and(|c| aggregate_failures(cx, &c, true)) {
            return;
        }
        // find_expectation: expectations, and aggregate_failures blocks (counted once, not entered)
        struct V<'a> {
            l: &'a helpers::rspec::Language,
            count: i64,
        }
        impl<'pr> Visit<'pr> for V<'_> {
            fn visit_call_node(&mut self, n: &CallNode<'pr>) {
                if n.receiver().is_none() && method_name(n) == "aggregate_failures" && helpers::rspec::plain_block_call(&n.as_node()).is_some() {
                    self.count += 1;
                    return;
                }
                if n.receiver().is_none() && self.l.expectation(method_name(n)) {
                    self.count += 1;
                }
                ruby_prism::visit_call_node(self, n);
            }
        }
        let mut v = V { l: &l, count: 0 };
        v.visit(node);
        // max_expectations: `Integer(cop_config.fetch('Max', 1))`, after the count
        let max = cx.cfg.integer_fetch("Max", 1);
        if v.count <= max {
            return;
        }
        let (s, e) = send_range(&call);
        cx.exclude_limit("Max", v.count);
        cx.add(s, e, format!("Example has too many expectations [{}/{max}].", v.count));
    }
}
