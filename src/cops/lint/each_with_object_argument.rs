use crate::prelude::*;

pub const NAME: &str = "Lint/EachWithObjectArgument";
pub static COP: EachWithObjectArgument = EachWithObjectArgument;
pub struct EachWithObjectArgument;

impl Cop for EachWithObjectArgument {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if method_name(&call) != "each_with_object" {
            return;
        }
        let args = send_args(&call);
        let [arg] = args.as_slice() else { return };
        // immutable_literal?: int float sym dsym true false nil complex rational
        let immutable = matches!(
            crate::helpers::literal_node::folded_kind(arg),
            kind::INTEGER_NODE
                | kind::FLOAT_NODE
                | kind::SYMBOL_NODE
                | kind::INTERPOLATED_SYMBOL_NODE
                | kind::TRUE_NODE
                | kind::FALSE_NODE
                | kind::NIL_NODE
                | kind::IMAGINARY_NODE
                | kind::RATIONAL_NODE
                | kind::SOURCE_LINE_NODE
        );
        if immutable {
            let (start, end) = send_range(&call);
            cx.add(start, end, "The argument to each_with_object cannot be immutable.");
        }
    }
}
