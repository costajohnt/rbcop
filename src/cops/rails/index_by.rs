use crate::prelude::*;

pub const NAME: &str = "Rails/IndexBy";
pub static COP: IndexBy = IndexBy;
pub struct IndexBy;

impl Cop for IndexBy {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        helpers::index_method::check(cx, node, helpers::index_method::Kind::By, "index_by");
    }
}
