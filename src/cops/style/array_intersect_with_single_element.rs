use crate::prelude::*;

pub const NAME: &str = "Style/ArrayIntersectWithSingleElement";
pub static COP: ArrayIntersectWithSingleElement = ArrayIntersectWithSingleElement;
pub struct ArrayIntersectWithSingleElement;

impl Cop for ArrayIntersectWithSingleElement {
    fn correctable(&self) -> bool {
        true
    }
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        // `(send _ _ $(array $_))`: a plain send with one array argument holding one element.
        if method_name(&call) != "intersect?"
            || is_csend(&call)
            || call.block().is_some_and(|b| b.as_block_argument_node().is_some())
        {
            return;
        }
        let args = call_args(&call);
        let [array] = args.as_slice() else { return };
        let Some(arr) = array.as_array_node() else { return };
        let elements: Vec<Node> = arr.elements().iter().collect();
        let [element] = elements.as_slice() else { return };
        if element.as_splat_node().is_some() {
            return;
        }
        let Some(sel) = call.message_loc() else { return };
        let percent = arr.opening_loc().is_some_and(|o| text(&o).starts_with('%'));
        let replacement = if percent {
            if let Some(s) = element.as_string_node() {
                if cx.src.binary {
                    helpers::ruby_string::inspect_binary(s.unescaped())
                } else {
                    helpers::ruby_string::inspect(s.unescaped())
                }
            } else if let Some(s) = element.as_symbol_node() {
                helpers::ruby_string::symbol_inspect(s.unescaped())
            } else if let Some(d) = element.as_interpolated_string_node() {
                // DstrNode#value: child values, or the source of children without one.
                let mut value = Vec::new();
                for part in d.parts().iter() {
                    if let Some(s) = part.as_string_node() {
                        value.extend_from_slice(s.unescaped());
                    } else if let Some(v) = part.as_embedded_variable_node() {
                        value.extend_from_slice(node_text(&v.variable()).as_bytes());
                    } else {
                        value.extend_from_slice(node_text(&part).as_bytes());
                    }
                }
                helpers::ruby_string::inspect(&value)
            } else {
                // dsym `value` raises: RuboCop's corrector crashes and drops the offense.
                return;
            }
        } else {
            node_text(element).to_string()
        };
        let (_, end) = send_range(&call);
        let (as_, ae) = (array.location().start_offset(), array.location().end_offset());
        let (ss, se) = (sel.start_offset(), sel.end_offset());
        cx.add_fix(ss, end, "Use `include?(element)` instead of `intersect?([element])`.", |c| {
            c.replace(ss, se, "include?");
            c.replace(as_, ae, &replacement);
        });
    }
}
