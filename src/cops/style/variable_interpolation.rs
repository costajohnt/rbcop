use crate::prelude::*;

pub const NAME: &str = "Style/VariableInterpolation";
pub static COP: VariableInterpolation = VariableInterpolation;
pub struct VariableInterpolation;

impl Cop for VariableInterpolation {
    fn correctable(&self) -> bool {
        true
    }

    // ponytail: every `#@var` part is a variable child of some dstr/dsym/xstr/regexp in parser.
    fn kinds(&self) -> &'static [usize] {
        &[kind::EMBEDDED_VARIABLE_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let var = node.as_embedded_variable_node().unwrap().variable();
        let loc = var.location();
        let (start, end) = (loc.start_offset(), loc.end_offset());
        let source = cx.src.slice(start, end);
        let msg = format!("Replace interpolated variable `{source}` with expression `#{{{source}}}`.");
        let replacement = format!("{{{source}}}");
        cx.add_fix(start, end, msg, |c| c.replace(start, end, &replacement));
    }
}
