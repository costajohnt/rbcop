use crate::prelude::*;
use helpers::range_help::{range_with_surrounding_space, Side, Surrounding};

pub const NAME: &str = "Lint/TrailingCommaInAttributeDeclaration";
pub static COP: TrailingCommaInAttributeDeclaration = TrailingCommaInAttributeDeclaration;
pub struct TrailingCommaInAttributeDeclaration;

impl Cop for TrailingCommaInAttributeDeclaration {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if call.receiver().is_some()
            || is_csend(&call)
            || !matches!(method_name(&call), "attr_reader" | "attr_writer" | "attr_accessor" | "attr")
        {
            return;
        }
        let args = send_args(&call);
        if args.len() < 2 || args.last().unwrap().as_def_node().is_none() {
            return;
        }
        let prev = args[args.len() - 2].location();
        let opts = Surrounding { side: Side::Right, ..Default::default() };
        let (_, e) = range_with_surrounding_space(cx.src.text, prev.start_offset(), prev.end_offset(), opts);
        cx.add_fix(e, e + 1, "Avoid leaving a trailing comma in attribute declarations.", |c| c.remove(e, e + 1));
    }
}
