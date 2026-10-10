use crate::prelude::*;

pub const NAME: &str = "Lint/EmptyClass";
pub static COP: EmptyClass = EmptyClass;
pub struct EmptyClass;

impl Cop for EmptyClass {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CLASS_NODE, kind::SINGLETON_CLASS_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let (body, msg) = match node.as_class_node() {
            Some(c) if c.superclass().is_some() => return,
            Some(c) => (c.body(), "Empty class detected."),
            None => (node.as_singleton_class_node().unwrap().body(), "Empty metaclass detected."),
        };
        if body.is_some() {
            return;
        }
        let l = node.location();
        let (start, end) = (l.start_offset(), l.end_offset());
        if cx.cfg.bool("AllowComments").unwrap_or(false) {
            let (first, last) = (cx.src.line_of(start), cx.src.line_of(end));
            if cx.src.comments.iter().any(|c| (first..=last).contains(&cx.src.line_of(c.start))) {
                return;
            }
        }
        cx.add(start, end, msg);
    }
}
