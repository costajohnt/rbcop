use crate::prelude::*;

pub const NAME: &str = "Metrics/CyclomaticComplexity";
pub static COP: CyclomaticComplexity = CyclomaticComplexity;
pub struct CyclomaticComplexity;

impl Cop for CyclomaticComplexity {
    fn kinds(&self) -> &'static [usize] {
        &[kind::DEF_NODE, kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        helpers::method_complexity::check_score(cx, node, false, "Cyclomatic");
    }
}
