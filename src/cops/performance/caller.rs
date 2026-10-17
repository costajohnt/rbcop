use crate::prelude::*;

pub const NAME: &str = "Performance/Caller";
pub static COP: Caller = Caller;
pub struct Caller;

impl Cop for Caller {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if is_csend(&call) {
            return;
        }
        let args = send_args(&call);
        // (send #slow_caller? :first) / (send #slow_caller? :[] int)
        let index = match (method_name(&call), args.as_slice()) {
            ("first", []) => None,
            ("[]", [i]) if i.as_integer_node().is_some() => int_literal_value(i),
            _ => return,
        };
        // (send nil? {:caller :caller_locations} int?)
        let Some(slow) = call.receiver().and_then(|r| r.as_call_node()) else { return };
        let name = method_name(&slow);
        if slow.receiver().is_some() || (name != "caller" && name != "caller_locations") || slow.block().is_some() {
            return;
        }
        let n = match send_args(&slow).as_slice() {
            [] => 1,
            [a] if a.as_integer_node().is_some() => match int_literal_value(a) {
                Some(v) => v,
                None => return,
            },
            _ => return,
        };
        let n = n + index.unwrap_or(0);
        let preferred = format!("{name}({n}..{n}).first");
        let (start, end) = send_range(&call);
        let msg = format!("Use `{preferred}` instead of `{}`.", cx.src.slice(start, end));
        cx.add_fix(start, end, msg, |c| c.replace(start, end, &preferred));
    }
}
