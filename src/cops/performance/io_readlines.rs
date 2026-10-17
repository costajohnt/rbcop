use crate::prelude::*;

pub const NAME: &str = "Performance/IoReadlines";
pub static COP: IoReadlines = IoReadlines;
pub struct IoReadlines;

/// `Enumerable.instance_methods + [:each]` on Ruby 4.0.6.
#[rustfmt::skip]
const ENUMERABLE: &[&str] = &[
    "all?", "any?", "chain", "chunk", "chunk_while", "collect", "collect_concat", "compact", "count", "cycle",
    "detect", "drop", "drop_while", "each", "each_cons", "each_entry", "each_slice", "each_with_index",
    "each_with_object", "entries", "filter", "filter_map", "find", "find_all", "find_index", "first", "flat_map",
    "grep", "grep_v", "group_by", "include?", "inject", "lazy", "map", "max", "max_by", "member?", "min", "min_by",
    "minmax", "minmax_by", "none?", "one?", "partition", "reduce", "reject", "reverse_each", "select",
    "slice_after", "slice_before", "slice_when", "sort", "sort_by", "sum", "take", "take_while", "tally", "to_a",
    "to_h", "to_set", "uniq", "zip",
];

impl Cop for IoReadlines {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let enumerable = node.as_call_node().unwrap();
        let name = method_name(&enumerable);
        if is_csend(&enumerable) || !ENUMERABLE.contains(&name) {
            return;
        }
        let Some(readlines) = enumerable.receiver().and_then(|r| r.as_call_node()) else { return };
        if is_csend(&readlines) || method_name(&readlines) != "readlines" || readlines.block().is_some_and(|b| b.as_block_node().is_some()) {
            return;
        }
        let io_class = readlines.receiver().is_some_and(|r| {
            r.as_constant_read_node().is_some_and(|c| matches!(c.name().as_slice(), b"IO" | b"File"))
        });
        // readlines_on_class? (no arguments on the enumerable call), else readlines_on_instance?
        // (any receiver but a constant). Only an instance receiver gets a correction.
        let correctable = if io_class && send_args(&enumerable).is_empty() {
            false
        } else {
            let recv = readlines.receiver();
            if recv.as_ref().is_some_and(|r| matches!(kind::of(r), kind::CONSTANT_READ_NODE | kind::CONSTANT_PATH_NODE)) {
                return;
            }
            recv.is_some()
        };
        let start = readlines.message_loc().unwrap().start_offset();
        let sel_end = enumerable.message_loc().map_or(send_range(&enumerable).1, |l| l.end_offset());
        let good = if name == "each" { "each_line".to_string() } else { format!("each_line.{name}") };
        let msg = format!("Use `{good}` instead of `readlines.{name}`.");
        let args: Vec<&str> = send_args(&readlines).iter().map(|a| node_text(a)).collect();
        let replacement = if args.is_empty() { "each_line".to_string() } else { format!("each_line({})", args.join(", ")) };
        let end = if name == "each" {
            send_range(&enumerable).1
        } else {
            enumerable.call_operator_loc().map_or(sel_end, |d| d.start_offset())
        };
        cx.add_fix(start, sel_end, msg, |c| {
            if correctable {
                c.replace(start, end, &replacement);
            }
        });
    }
}
