use crate::prelude::*;

pub const NAME: &str = "Layout/EmptyLinesAroundClassBody";
pub static COP: EmptyLinesAroundClassBody = EmptyLinesAroundClassBody;
pub struct EmptyLinesAroundClassBody;

impl Cop for EmptyLinesAroundClassBody {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CLASS_NODE, kind::SINGLETON_CLASS_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let loc = node.location();
        let (body, first_line) = match node.as_class_node() {
            Some(c) => (c.body(), c.superclass().map(|s| cx.src.line_of(s.location().end_offset() - 1))),
            None => (node.as_singleton_class_node().unwrap().body(), None),
        };
        let body = helpers::empty_lines_around_body::body_children(body);
        let style = cx.cfg.str("EnforcedStyle").unwrap_or("no_empty_lines").to_string();
        let range = (loc.start_offset(), loc.end_offset());
        helpers::empty_lines_around_body::check(cx, "class", &style, range, body.as_deref(), first_line);
    }
}
