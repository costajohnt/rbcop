use crate::prelude::*;

pub const NAME: &str = "Style/HashTransformValues";
pub static COP: HashTransformValues = HashTransformValues;
pub struct HashTransformValues;

impl Cop for HashTransformValues {
    fn correctable(&self) -> bool {
        true
    }

    fn ruby_range(&self) -> (f64, f64) {
        (2.4, f64::MAX)
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        helpers::hash_transform_method::check(cx, node, false);
    }
}
