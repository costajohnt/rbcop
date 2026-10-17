use crate::prelude::*;

pub const NAME: &str = "RSpec/Eq";
pub static COP: Eq = Eq;
pub struct Eq;

impl Cop for Eq {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (send _ #Runners.all $(send (send nil? :be) :== _))
        let call = node.as_call_node().unwrap();
        if is_csend(&call) || !helpers::rspec::is_runner(method_name(&call)) {
            return;
        }
        let [arg] = &send_args(&call)[..] else { return };
        let Some(matcher) = helpers::rspec::plain_send(arg, "==") else { return };
        if send_args(&matcher).len() != 1 || matcher.receiver().and_then(|r| helpers::rspec::bare_send(&r, "be")).is_none() {
            return;
        }
        let s = matcher.location().start_offset();
        let e = matcher.message_loc().unwrap().end_offset();
        cx.add_fix(s, e, "Use `eq` instead of `be ==` to compare objects.", |c| c.replace(s, e, "eq"));
    }
}
