use crate::helpers::sorbet_rbs_assertion::t_send;
use crate::prelude::*;

pub const NAME: &str = "Sorbet/ForbidTAnyWithNil";
pub static COP: ForbidTAnyWithNil = ForbidTAnyWithNil;
pub struct ForbidTAnyWithNil;

impl Cop for ForbidTAnyWithNil {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (send (const nil? :T) :any $...)
        let Some(call) = t_send(node, "any") else { return };
        let args = send_args(&call);
        let is_nil = |a: &Node| a.as_constant_read_node().is_some_and(|c| c.name().as_slice() == b"NilClass");
        let sources: Vec<&str> = args.iter().filter(|a| !is_nil(a)).map(node_text).collect();
        if sources.len() == args.len() || sources.is_empty() {
            return;
        }
        let inner = if sources.len() == 1 { sources[0].to_string() } else { format!("T.any({})", sources.join(", ")) };
        let (s, e) = send_range(&call);
        cx.add_fix(s, e, "Use `T.nilable` instead of `T.any(..., NilClass, ...)`.", |c| {
            c.replace(s, e, &format!("T.nilable({inner})"))
        });
    }
}
