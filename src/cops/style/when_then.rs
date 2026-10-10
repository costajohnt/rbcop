use crate::prelude::*;

pub const NAME: &str = "Style/WhenThen";
pub static COP: WhenThen = WhenThen;
pub struct WhenThen;

impl Cop for WhenThen {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::WHEN_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let when = node.as_when_node().unwrap();
        if when.then_keyword_loc().is_some() {
            return;
        }
        let Some(body) = when.statements() else { return };
        let start = when.keyword_loc().start_offset();
        if cx.src.line_of(start) != cx.src.line_of(body.location().end_offset()) {
            return;
        }
        let conds: Vec<Node> = when.conditions().iter().collect();
        let Some(last) = conds.last() else { return };
        // parser `loc.begin` is the `;` between the conditions and the body.
        let from = last.location().end_offset();
        let Some(semi) = cx.src.text[from..body.location().start_offset()].find(';').map(|i| from + i) else {
            return;
        };
        let expr = conds.iter().map(node_text).collect::<Vec<_>>().join(", ");
        let msg = format!("Do not use `when {expr};`. Use `when {expr} then` instead.");
        cx.add_fix(semi, semi + 1, msg, |c| c.replace(semi, semi + 1, " then"));
    }
}
