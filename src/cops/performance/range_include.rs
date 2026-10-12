use crate::prelude::*;

pub const NAME: &str = "Performance/RangeInclude";
pub static COP: RangeInclude = RangeInclude;
pub struct RangeInclude;

impl Cop for RangeInclude {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (call {range (begin range)} ${:include? :member?} ...)
        let call = node.as_call_node().unwrap();
        let bad = method_name(&call);
        if bad != "include?" && bad != "member?" {
            return;
        }
        let Some(recv) = call.receiver() else { return };
        let range = recv.as_range_node().is_some()
            || recv.as_parentheses_node().and_then(|p| p.body()).and_then(|b| b.as_statements_node()).is_some_and(|s| {
                let body: Vec<Node> = s.body().iter().collect();
                matches!(body.as_slice(), [r] if r.as_range_node().is_some())
            });
        if !range {
            return;
        }
        let sel = call.message_loc().unwrap();
        let (start, end) = (sel.start_offset(), sel.end_offset());
        cx.add_fix(start, end, format!("Use `Range#cover?` instead of `Range#{bad}`."), |c| {
            c.replace(start, end, "cover?")
        });
    }
}
