use crate::prelude::*;
use helpers::minitest::*;

pub const NAME: &str = "Minitest/UnspecifiedException";
pub static COP: UnspecifiedException = UnspecifiedException;
pub struct UnspecifiedException;

impl Cop for UnspecifiedException {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let Some(call) = block_call(node) else { return };
        if method_name(&call) != "assert_raises" {
            return;
        }
        let args = send_args(&call);
        let str_arg = |n: &Node| {
            matches!(n, Node::StringNode { .. } | Node::SourceFileNode { .. }) && !is_parser_dstr(n)
        };
        if args.is_empty() || (args.len() == 1 && str_arg(&args[0])) {
            let (s, e) = send_range(&call);
            cx.add(s, e, "Specify the exception being captured.");
        }
    }
}
