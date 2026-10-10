use crate::prelude::*;
use helpers::line_break_corrector::{correct_trailing_body, first_part_of, trailing_body};

pub const NAME: &str = "Style/TrailingBodyOnModule";
pub static COP: TrailingBodyOnModule = TrailingBodyOnModule;
pub struct TrailingBodyOnModule;

impl Cop for TrailingBodyOnModule {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::MODULE_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let n = node.as_module_node().unwrap();
        let body = n.body();
        let src = cx.src;
        if !trailing_body(src, node, body.as_ref()) {
            return;
        }
        let body = body.unwrap();
        let keyword = n.module_keyword_loc().start_offset();
        let (s, e) = first_part_of(&body);
        let width = helpers::alignment::configured_indentation_width(cx);
        cx.add_fix(s, e, "Place the first line of module body on its own line.", |c| {
            correct_trailing_body(c, src, node, keyword, &body, width)
        });
    }
}
