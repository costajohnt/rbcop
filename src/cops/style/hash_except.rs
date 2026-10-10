use crate::prelude::*;

pub const NAME: &str = "Style/HashExcept";
pub static COP: HashExcept = HashExcept;
pub struct HashExcept;

impl Cop for HashExcept {
    fn correctable(&self) -> bool {
        true
    }

    fn ruby_range(&self) -> (f64, f64) {
        (3.0, f64::MAX)
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        helpers::hash_subset::check(cx, node, true);
    }
}
