use crate::helpers::element_line_breaks as elb;
use crate::prelude::*;

pub const NAME: &str = "Layout/FirstArrayElementLineBreak";
pub static COP: FirstArrayElementLineBreak = FirstArrayElementLineBreak;
pub struct FirstArrayElementLineBreak;

const MSG: &str = "Add a line break before the first element of a multi-line array.";

impl Cop for FirstArrayElementLineBreak {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::ARRAY_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let a = node.as_array_node().unwrap();
        let start = node.location().start_offset();
        if a.opening_loc().is_none() {
            // `assignment_on_same_line?`: the line before the array ends in `=`.
            let line_start = cx.src.line_start(cx.src.line_of(start));
            let before = cx.src.slice(line_start, start).trim_end_matches(|c: char| c.is_ascii_whitespace());
            if !before.ends_with('=') || cx.cfg.bool("AllowImplicitArrayLiterals").unwrap_or(false) {
                return;
            }
        }
        let children = elb::ranges(a.elements().iter());
        elb::check_children_line_break(cx, start, &children, elb::ignore_last(cx), MSG);
    }
}
