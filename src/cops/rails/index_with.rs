use crate::prelude::*;

pub const NAME: &str = "Rails/IndexWith";
pub static COP: IndexWith = IndexWith;
pub struct IndexWith;

impl Cop for IndexWith {
    fn correctable(&self) -> bool {
        true
    }

    fn min_rails(&self) -> f64 {
        6.0
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        helpers::index_method::check(cx, node, helpers::index_method::Kind::With, "index_with");
    }
}
