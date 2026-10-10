use crate::prelude::*;
use helpers::line_break_corrector::{correct_trailing_body, first_part_of, trailing_body};

pub const NAME: &str = "Style/TrailingBodyOnClass";
pub static COP: TrailingBodyOnClass = TrailingBodyOnClass;
pub struct TrailingBodyOnClass;

impl Cop for TrailingBodyOnClass {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CLASS_NODE, kind::SINGLETON_CLASS_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let (keyword, body) = match node.as_class_node() {
            Some(n) => (n.class_keyword_loc().start_offset(), n.body()),
            None => {
                let n = node.as_singleton_class_node().unwrap();
                (n.class_keyword_loc().start_offset(), n.body())
            }
        };
        let src = cx.src;
        if !trailing_body(src, node, body.as_ref()) {
            return;
        }
        let body = body.unwrap();
        let (s, e) = first_part_of(&body);
        let width = helpers::alignment::configured_indentation_width(cx);
        cx.add_fix(s, e, "Place the first line of class body on its own line.", |c| {
            correct_trailing_body(c, src, node, keyword, &body, width)
        });
    }
}
