use crate::prelude::*;

pub const NAME: &str = "Performance/ArraySemiInfiniteRangeSlice";
pub static COP: ArraySemiInfiniteRangeSlice = ArraySemiInfiniteRangeSlice;
pub struct ArraySemiInfiniteRangeSlice;

fn positive_int(node: &Node) -> Option<i128> {
    node.as_integer_node()?;
    int_literal_value(node).filter(|v| *v > 0)
}

impl Cop for ArraySemiInfiniteRangeSlice {
    fn correctable(&self) -> bool {
        true
    }

    fn ruby_range(&self) -> (f64, f64) {
        (2.7, f64::MAX)
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (call $!any_str $%SLICE_METHODS $#endless_range?)
        let call = node.as_call_node().unwrap();
        let name = method_name(&call);
        if name != "[]" && name != "slice" {
            return;
        }
        let args = send_args(&call);
        let [arg] = args.as_slice() else { return };
        let Some(range) = arg.as_range_node() else { return };
        // A receiver-less call matches, but the corrector raises on `nil.source`: no offense.
        let Some(recv) = call.receiver() else { return };
        if matches!(
            kind::of(&recv),
            kind::STRING_NODE
                | kind::INTERPOLATED_STRING_NODE
                | kind::X_STRING_NODE
                | kind::INTERPOLATED_X_STRING_NODE
                | kind::SOURCE_FILE_NODE
        ) {
            return;
        }
        let method_call = match (range.left(), range.right()) {
            (None, Some(r)) => match positive_int(&r) {
                Some(v) if range.is_exclude_end() => format!("take({v})"),
                Some(v) => format!("take({})", v + 1),
                None => return,
            },
            (Some(l), None) => match positive_int(&l) {
                Some(v) => format!("drop({v})"),
                None => return,
            },
            _ => return,
        };
        let prefer = if range.left().is_some() { "drop" } else { "take" };
        let msg = format!("Use `{prefer}` instead of `{name}` with semi-infinite range.");
        let replacement = format!("{}.{method_call}", node_text(&recv));
        let (start, end) = send_range(&call);
        cx.add_fix(start, end, msg, |c| c.replace(start, end, &replacement));
    }
}
