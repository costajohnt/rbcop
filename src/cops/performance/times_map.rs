use crate::prelude::*;

pub const NAME: &str = "Performance/TimesMap";
pub static COP: TimesMap = TimesMap;
pub struct TimesMap;

impl Cop for TimesMap {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let name = method_name(&call);
        if name != "map" && name != "collect" {
            return;
        }
        // (call (call $!nil? :times) {:map :collect} ...)
        let Some(times) = call.receiver().and_then(|r| r.as_call_node()) else { return };
        if method_name(&times) != "times" || !send_args(&times).is_empty() || times.block().is_some() {
            return;
        }
        let Some(count) = times.receiver() else { return };
        let args = send_args(&call);
        // The offense is on the block (any_block), or on the call with a block pass.
        let (start, end) = match call.block() {
            Some(b) if b.as_block_node().is_some() && args.is_empty() => {
                (node.location().start_offset(), node.location().end_offset())
            }
            Some(b) if b.as_block_argument_node().is_some() && args.len() == 1 => send_range(&call),
            _ => return,
        };
        // handleable_receiver?: a numeric literal count, or `.times` called with a dot.
        let numeric = matches!(kind::of(&count), kind::INTEGER_NODE | kind::FLOAT_NODE);
        if !numeric && times.call_operator_loc().is_none_or(|d| d.as_slice() != b".") {
            return;
        }
        let count_src = node_text(&count);
        let only_if = if helpers::literal_node::literal(&count) {
            String::new()
        } else {
            format!(" only if `{count_src}` is always 0 or more")
        };
        let msg = format!("Use `Array.new({count_src})` with a block instead of `.times.{name}`{only_if}.");
        let extra: String = args.iter().map(|a| format!(", {}", node_text(a))).collect();
        let replacement = format!("Array.new({count_src}{extra})");
        let (s, e) = send_range(&call);
        cx.add_fix(start, end, msg, |c| c.replace(s, e, &replacement));
    }
}
