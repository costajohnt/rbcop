use crate::prelude::*;

pub const NAME: &str = "Lint/MixedRegexpCaptureTypes";
pub static COP: MixedRegexpCaptureTypes = MixedRegexpCaptureTypes;
pub struct MixedRegexpCaptureTypes;

impl Cop for MixedRegexpCaptureTypes {
    fn kinds(&self) -> &'static [usize] {
        &[kind::REGULAR_EXPRESSION_NODE, kind::INTERPOLATED_REGULAR_EXPRESSION_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        if let Some((named, numbered)) = helpers::regexp_captures::regexp_captures(node) {
            if named > 0 && numbered > 0 {
                let loc = node.location();
                cx.add(
                    loc.start_offset(),
                    loc.end_offset(),
                    "Do not mix named captures and numbered captures in a Regexp literal.",
                );
            }
        }
    }
}
