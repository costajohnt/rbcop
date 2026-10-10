use crate::prelude::*;

pub const NAME: &str = "Style/HashSlice";
pub static COP: HashSlice = HashSlice;
pub struct HashSlice;

impl Cop for HashSlice {
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
        helpers::hash_subset::check(cx, node, false);
    }
}
