use crate::prelude::*;

pub const NAME: &str = "RSpec/EmptyHook";
pub static COP: EmptyHook = EmptyHook;
pub struct EmptyHook;

impl Cop for EmptyHook {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (block $(send nil? #Hooks.all ...) _ nil?)
        let Some(call) = helpers::rspec::plain_block_call(node) else { return };
        let l = helpers::rspec::lang(cx);
        if call.receiver().is_some() || !l.hook(method_name(&call)) || helpers::rspec::block_body(&call).is_some() {
            return;
        }
        let (ss, se) = send_range(&call);
        let (ns, ne) = helpers::rspec::node_range(node);
        let opts = helpers::range_help::Surrounding { side: helpers::range_help::Side::Left, ..Default::default() };
        let (s, e) = helpers::range_help::range_with_surrounding_space(cx.src.text, ns, ne, opts);
        cx.add_fix(ss, se, "Empty hook detected.", |c| c.remove(s, e));
    }
}
