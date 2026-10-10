use crate::prelude::*;

pub const NAME: &str = "Metrics/PerceivedComplexity";
pub static COP: PerceivedComplexity = PerceivedComplexity;
pub struct PerceivedComplexity;

impl Cop for PerceivedComplexity {
    fn kinds(&self) -> &'static [usize] {
        &[kind::DEF_NODE, kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        helpers::method_complexity::check_score(cx, node, true, "Perceived");
    }
}
