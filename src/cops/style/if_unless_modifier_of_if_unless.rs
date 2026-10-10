use crate::helpers::if_node;
use crate::prelude::*;

pub const NAME: &str = "Style/IfUnlessModifierOfIfUnless";
pub static COP: IfUnlessModifierOfIfUnless = IfUnlessModifierOfIfUnless;
pub struct IfUnlessModifierOfIfUnless;

impl Cop for IfUnlessModifierOfIfUnless {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::IF_NODE, kind::UNLESS_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        if is_pattern_guard(node, cx.ancestors) {
            return;
        }
        let Some(c) = if_node::cond(node) else { return };
        let Some(body) = c.body.as_ref() else { return };
        if !c.modifier() || !matches!(body, Node::IfNode { .. } | Node::UnlessNode { .. }) {
            return;
        }
        let (ks, ke) = c.keyword_loc.unwrap();
        let b = body.location();
        let (bs, be) = (b.start_offset(), b.end_offset());
        let cond_end = c.condition.location().end_offset();
        let before = format!("{} {}\n", c.keyword, node_text(&c.condition));
        let msg = format!("Avoid modifier `{}` after another conditional.", c.keyword);
        cx.add_fix(ks, ke, msg, |corr| {
            corr.wrap(bs, be, &before, "\nend");
            corr.remove(be, cond_end);
        });
    }
}
