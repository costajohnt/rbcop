use crate::prelude::*;

pub const NAME: &str = "Performance/ReverseFirst";
pub static COP: ReverseFirst = ReverseFirst;
pub struct ReverseFirst;

impl Cop for ReverseFirst {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (call $(call _ :reverse) :first (int _)?)
        let call = node.as_call_node().unwrap();
        if method_name(&call) != "first" {
            return;
        }
        let args = send_args(&call);
        if args.len() > 1 || args.first().is_some_and(|a| a.as_integer_node().is_none()) {
            return;
        }
        let Some(reverse) = call.receiver().and_then(|r| r.as_call_node()) else { return };
        if method_name(&reverse) != "reverse" || !send_args(&reverse).is_empty() || reverse.block().is_some() {
            return;
        }
        let start = reverse.message_loc().unwrap().start_offset();
        let end = send_range(&call).1;
        let good = match (args.first(), call.call_operator_loc()) {
            (Some(a), Some(dot)) => format!("last({}){}reverse", node_text(a), text(&dot)),
            _ => "last".to_string(),
        };
        let msg = format!("Use `{good}` instead of `{}`.", cx.src.slice(start, end));
        cx.add_fix(start, end, msg, |c| c.replace(start, end, &good));
    }
}
