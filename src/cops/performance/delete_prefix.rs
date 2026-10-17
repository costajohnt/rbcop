use crate::helpers::regexp_metacharacter::{drop_start, literal_at_start};
use crate::prelude::*;

pub const NAME: &str = "Performance/DeletePrefix";
pub static COP: DeletePrefix = DeletePrefix;
pub struct DeletePrefix;

impl Cop for DeletePrefix {
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
        helpers::regexp_metacharacter::delete_affix(cx, node, "delete_prefix", literal_at_start, drop_start);
    }
}
