use crate::prelude::*;

pub const NAME: &str = "Layout/ConditionPosition";
pub static COP: ConditionPosition = ConditionPosition;
pub struct ConditionPosition;

impl Cop for ConditionPosition {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::IF_NODE, kind::UNLESS_NODE, kind::WHILE_NODE, kind::UNTIL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let (kw, cond, body) = match kind::of(node) {
            kind::IF_NODE => {
                let n = node.as_if_node().unwrap();
                let Some(kw) = n.if_keyword_loc() else { return }; // ternary
                (kw, n.predicate(), n.statements())
            }
            kind::UNLESS_NODE => {
                let n = node.as_unless_node().unwrap();
                (n.keyword_loc(), n.predicate(), n.statements())
            }
            kind::WHILE_NODE => {
                let n = node.as_while_node().unwrap();
                (n.keyword_loc(), n.predicate(), n.statements())
            }
            _ => {
                let n = node.as_until_node().unwrap();
                (n.keyword_loc(), n.predicate(), n.statements())
            }
        };
        // modifier_form?: the body comes before the keyword.
        if node.location().start_offset() != kw.start_offset() || is_pattern_guard(node, cx.ancestors) {
            return;
        }
        let loc = cond.location();
        if cx.src.line_of(kw.start_offset()) == cx.src.line_of(loc.start_offset()) {
            return;
        }
        let msg = format!("Place the condition on the same line as `{}`.", text(&kw));
        let (start, end) = (loc.start_offset(), loc.end_offset());
        let src = cx.src;
        // removal_range: up to the body when it starts on the condition's last line, else whole lines.
        let removal = match body.map(|b| b.location().start_offset()) {
            Some(b) if src.line_of(b) == src.line_of(end) => (start, b),
            _ => helpers::whole_lines::range_by_whole_lines(src.text, start, end, true),
        };
        let insert = format!(" {}", src.slice(start, end));
        let (kw_start, kw_end) = (kw.start_offset(), kw.end_offset());
        cx.add_fix(start, end, msg, |c| {
            c.insert_after(kw_start, kw_end, &insert);
            c.remove(removal.0, removal.1);
        });
    }
}

