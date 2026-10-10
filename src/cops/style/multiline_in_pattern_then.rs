use crate::helpers::range_help::{Side, Surrounding, range_with_surrounding_space};
use crate::prelude::*;

pub const NAME: &str = "Style/MultilineInPatternThen";
pub static COP: MultilineInPatternThen = MultilineInPatternThen;
pub struct MultilineInPatternThen;

impl Cop for MultilineInPatternThen {
    fn correctable(&self) -> bool {
        true
    }

    fn ruby_range(&self) -> (f64, f64) {
        (2.7, f64::MAX)
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::IN_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let in_node = node.as_in_node().unwrap();
        let Some(then) = in_node.then_loc().filter(|t| t.as_slice() == b"then") else { return };
        // Prism wraps a guarded pattern (`in pat if cond`) in an IfNode/UnlessNode.
        let pat = in_node.pattern();
        let pattern = match (pat.as_if_node(), pat.as_unless_node()) {
            (Some(i), _) => i.statements().and_then(|s| s.body().iter().next()),
            (_, Some(u)) => u.statements().and_then(|s| s.body().iter().next()),
            _ => Some(pat),
        };
        let Some(pattern) = pattern else { return };
        let ploc = pattern.location();
        let src = cx.src;
        if src.line_of(ploc.start_offset()) != src.line_of(ploc.end_offset()) {
            return;
        }
        if let Some(body) = in_node.statements()
            && src.line_of(node.location().start_offset()) == src.line_of(body.location().start_offset()) {
                return;
            }
        let (s, e) = (then.start_offset(), then.end_offset());
        let opts = Surrounding { side: Side::Left, newlines: false, ..Default::default() };
        let (rs, re) = range_with_surrounding_space(src.text, s, e, opts);
        cx.add_fix(s, e, "Do not use `then` for multiline `in` statement.", |c| c.remove(rs, re));
    }
}
