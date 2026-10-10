use crate::helpers::method_complexity::{format_g4, max, target};
use crate::prelude::*;

pub const NAME: &str = "Metrics/AbcSize";
pub static COP: AbcSize = AbcSize;
pub struct AbcSize;

impl Cop for AbcSize {
    fn kinds(&self) -> &'static [usize] {
        &[kind::DEF_NODE, kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let Some((name, body, (start, end))) = target(cx, node) else { return };
        let discount = !cx.cfg.bool("CountRepeatedAttributes").unwrap_or(false);
        let (size, vector) = helpers::abc_size_calculator::calculate(&body, discount);
        let max = max(cx);
        if size <= max {
            return;
        }
        let msg = format!(
            "Assignment Branch Condition size for `{name}` is too high. [{vector} {}/{}]",
            format_g4(size),
            format_g4(max)
        );
        cx.add(start, end, msg);
    }
}
