use crate::helpers::regexp_metacharacter::{drop_end, literal_at_end};
use crate::prelude::*;

pub const NAME: &str = "Performance/DeleteSuffix";
pub static COP: DeleteSuffix = DeleteSuffix;
pub struct DeleteSuffix;

impl Cop for DeleteSuffix {
    fn correctable(&self) -> bool {
        true
    }

    fn ruby_range(&self) -> (f64, f64) {
        (2.5, f64::MAX)
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        helpers::regexp_metacharacter::delete_affix(cx, node, "delete_suffix", literal_at_end, drop_end);
    }
}
