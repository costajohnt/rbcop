use crate::prelude::*;

pub const NAME: &str = "Lint/RegexpAsCondition";
pub static COP: RegexpAsCondition = RegexpAsCondition;
pub struct RegexpAsCondition;

impl Cop for RegexpAsCondition {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::MATCH_LAST_LINE_NODE, kind::INTERPOLATED_MATCH_LAST_LINE_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let conditional = cx.ancestors.iter().any(|a| {
            matches!(
                kind::of(a),
                kind::IF_NODE | kind::UNLESS_NODE | kind::WHILE_NODE | kind::UNTIL_NODE | kind::CASE_NODE | kind::CASE_MATCH_NODE
            )
        });
        if !conditional {
            return;
        }
        // ponytail: part_of_ignored_node? only matters for a match_current_line nested in another's
        // interpolation; not tracked.
        let loc = node.location();
        let (s, e) = (loc.start_offset(), loc.end_offset());
        let src = node_text(node);
        let not_parent = cx
            .parent()
            .and_then(|p| p.as_call_node())
            .filter(|c| method_name(c) == "!")
            .map(|c| {
                let l = c.location();
                (l.start_offset(), l.end_offset())
            });
        let msg = "Do not use regexp literal as a condition. The regexp literal matches `$_` implicitly.";
        cx.add_fix(s, e, msg, |c| match not_parent {
            Some((ps, pe)) => c.replace(ps, pe, &format!("!({src} =~ $_)")),
            None => c.replace(s, e, &format!("{src} =~ $_")),
        });
    }
}
