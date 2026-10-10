use crate::prelude::*;

pub const NAME: &str = "Layout/BeginEndAlignment";
pub static COP: BeginEndAlignment = BeginEndAlignment;
pub struct BeginEndAlignment;

impl Cop for BeginEndAlignment {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::BEGIN_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let b = node.as_begin_node().unwrap();
        let (Some(begin), Some(end)) = (b.begin_keyword_loc(), b.end_keyword_loc()) else { return };
        let style = cx.cfg.str("EnforcedStyleAlignWith").unwrap_or("start_of_line").to_string();
        let ranges = [
            ("begin", (begin.start_offset(), begin.end_offset())),
            ("start_of_line", helpers::end_keyword_alignment::start_line_range(cx.src, node.location().start_offset())),
        ];
        helpers::end_keyword_alignment::check(cx, (end.start_offset(), end.end_offset()), &ranges, &style);
    }
}
