use crate::prelude::*;

pub const NAME: &str = "Style/TrailingMethodEndStatement";
pub static COP: TrailingMethodEndStatement = TrailingMethodEndStatement;
pub struct TrailingMethodEndStatement;

impl Cop for TrailingMethodEndStatement {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::DEF_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let def = node.as_def_node().unwrap();
        let (Some(end), Some(body)) = (def.end_keyword_loc(), def.body()) else { return };
        if def.equal_loc().is_some() {
            return;
        }
        let loc = node.location();
        let line = |o: usize| cx.src.line_of(o);
        if line(loc.start_offset()) == line(loc.end_offset()) || line(body_range(&body).1) != line(end.end_offset()) {
            return;
        }
        let indent = " ".repeat(cx.src.column(def.def_keyword_loc().start_offset()));
        let (s, e) = (end.start_offset(), end.end_offset());
        cx.add_fix(s, e, "Place the end statement of a multi-line method on its own line.", |c| {
            c.insert_before(s, e, &format!("\n{indent}"))
        });
    }
}
