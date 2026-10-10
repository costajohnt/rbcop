use crate::prelude::*;

pub const NAME: &str = "Lint/EmptyEnsure";
pub static COP: EmptyEnsure = EmptyEnsure;
pub struct EmptyEnsure;

impl Cop for EmptyEnsure {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::ENSURE_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let ensure = node.as_ensure_node().unwrap();
        if ensure.statements().is_some() {
            return;
        }
        let kw = ensure.ensure_keyword_loc();
        let (s, e) = (kw.start_offset(), kw.end_offset());
        cx.add_fix(s, e, "Empty `ensure` block detected.", |c| c.remove(s, e));
    }
}
