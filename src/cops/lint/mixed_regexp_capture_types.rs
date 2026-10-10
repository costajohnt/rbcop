use crate::prelude::*;

pub const NAME: &str = "Lint/MixedRegexpCaptureTypes";
pub static COP: MixedRegexpCaptureTypes = MixedRegexpCaptureTypes;
pub struct MixedRegexpCaptureTypes;

impl Cop for MixedRegexpCaptureTypes {
    fn kinds(&self) -> &'static [usize] {
        &[kind::REGULAR_EXPRESSION_NODE, kind::INTERPOLATED_REGULAR_EXPRESSION_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let Some((named, numbered)) = helpers::regexp_captures::regexp_captures(node) else { return };
        if named > 0 && numbered > 0 {
            let loc = node.location();
            let msg = "Do not mix named captures and numbered captures in a Regexp literal.";
            cx.add(loc.start_offset(), loc.end_offset(), msg);
        }
    }
}
