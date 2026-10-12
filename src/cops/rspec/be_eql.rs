use crate::prelude::*;

pub const NAME: &str = "RSpec/BeEql";
pub static COP: BeEql = BeEql;
pub struct BeEql;

impl Cop for BeEql {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (send _ :to $(send nil? :eql {boolean int float sym nil}))
        let call = node.as_call_node().unwrap();
        if is_csend(&call) || method_name(&call) != "to" {
            return;
        }
        let [arg] = &send_args(&call)[..] else { return };
        let Some(eql) = helpers::rspec::nil_send(arg, "eql") else { return };
        let [value] = &send_args(&eql)[..] else { return };
        let ok = match kind::of(value) {
            kind::TRUE_NODE | kind::FALSE_NODE | kind::NIL_NODE | kind::INTEGER_NODE | kind::FLOAT_NODE => true,
            kind::SYMBOL_NODE => !helpers::rspec::is_dsym(cx, value),
            _ => false,
        };
        if !ok {
            return;
        }
        let sel = eql.message_loc().unwrap();
        let (s, e) = (sel.start_offset(), sel.end_offset());
        cx.add_fix(s, e, "Prefer `be` over `eql`.", |c| c.replace(s, e, "be"));
    }
}
