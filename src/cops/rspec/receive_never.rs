use crate::prelude::*;
use ruby_prism::Visit;

pub const NAME: &str = "RSpec/ReceiveNever";
pub static COP: ReceiveNever = ReceiveNever;
pub struct ReceiveNever;

/// expect_to_receive?: `(send {(send #rspec? {:expect :expect_any_instance_of} ...)
/// (block (send #rspec? :expect) ...) (send nil? :is_expected)} :to ...)`
fn expect_to_receive(call: &ruby_prism::CallNode) -> bool {
    if is_csend(call) || method_name(call) != "to" {
        return false;
    }
    let Some(recv) = call.receiver() else { return false };
    let Some(r) = recv.as_call_node() else { return false };
    let name = method_name(&r);
    match helpers::rspec::block_of(&r) {
        None => {
            (helpers::rspec::rspec_recv(&r) && matches!(name, "expect" | "expect_any_instance_of"))
                || (r.receiver().is_none() && name == "is_expected" && send_args(&r).is_empty())
        }
        Some(b) => helpers::rspec::is_plain(&b) && helpers::rspec::rspec_recv(&r) && name == "expect" && send_args(&r).is_empty(),
    }
}

impl Cop for ReceiveNever {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if is_csend(&call) || method_name(&call) != "never" {
            return;
        }
        // method_on_stub?: (send nil? :receive ...) in the send
        struct V(bool);
        impl<'pr> Visit<'pr> for V {
            fn visit_call_node(&mut self, n: &ruby_prism::CallNode<'pr>) {
                self.0 |= n.receiver().is_none() && method_name(n) == "receive";
                ruby_prism::visit_call_node(self, n);
            }
        }
        let mut v = V(false);
        if let Some(r) = call.receiver() {
            v.visit(&r);
        }
        for a in send_args(&call) {
            v.visit(&a);
        }
        if !v.0 {
            return;
        }
        // used_with_expect?: a `send` ancestor (not a call whose block holds the node)
        let anc = cx.ancestors;
        let used = (0..anc.len()).any(|i| {
            anc[i].as_call_node().is_some_and(|c| {
                let owns_block = anc.get(i + 1).is_some_and(|n| n.as_block_node().is_some());
                !owns_block && expect_to_receive(&c)
            })
        });
        if !used {
            return;
        }
        let sel = call.message_loc().unwrap();
        let (s, e) = (sel.start_offset(), sel.end_offset());
        // node.parent.loc.selector (RuboCop raises without one) and `.never`
        let parent = helpers::parser_parent::parser_parent(anc).and_then(|p| p.as_call_node());
        let Some(psel) = parent.and_then(|p| p.message_loc()) else { return };
        let (ps, pe) = (psel.start_offset(), psel.end_offset());
        let Some(dot) = call.call_operator_loc() else { return };
        let ds = dot.start_offset();
        cx.add_fix(s, e, "Use `not_to receive` instead of `never`.", |c| {
            c.replace(ps, pe, "not_to");
            c.remove(ds, e);
        });
    }
}
