use crate::prelude::*;

pub const NAME: &str = "Layout/EmptyLinesAroundModuleBody";
pub static COP: EmptyLinesAroundModuleBody = EmptyLinesAroundModuleBody;
pub struct EmptyLinesAroundModuleBody;

impl Cop for EmptyLinesAroundModuleBody {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::MODULE_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let loc = node.location();
        let body = helpers::empty_lines_around_body::body_children(node.as_module_node().unwrap().body());
        let style = cx.cfg.str("EnforcedStyle").unwrap_or("no_empty_lines").to_string();
        let range = (loc.start_offset(), loc.end_offset());
        helpers::empty_lines_around_body::check(cx, "module", &style, range, body.as_deref(), None);
    }
}
