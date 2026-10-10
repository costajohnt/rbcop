use crate::prelude::*;

pub const NAME: &str = "Layout/EmptyLinesAroundBeginBody";
pub static COP: EmptyLinesAroundBeginBody = EmptyLinesAroundBeginBody;
pub struct EmptyLinesAroundBeginBody;

impl Cop for EmptyLinesAroundBeginBody {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::BEGIN_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        if node.as_begin_node().unwrap().begin_keyword_loc().is_none() {
            return;
        }
        let loc = node.location();
        let range = (loc.start_offset(), loc.end_offset());
        helpers::empty_lines_around_body::check(cx, "`begin`", "no_empty_lines", range, None, None);
    }
}
