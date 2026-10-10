use crate::prelude::*;

pub const NAME: &str = "Layout/EmptyLinesAroundMethodBody";
pub static COP: EmptyLinesAroundMethodBody = EmptyLinesAroundMethodBody;
pub struct EmptyLinesAroundMethodBody;

impl Cop for EmptyLinesAroundMethodBody {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::DEF_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let def = node.as_def_node().unwrap();
        let src = cx.src;
        if let Some(eq) = def.equal_loc() {
            // Endless method: an empty line right after `=` before a body on a later line.
            let eq_line = src.line_of(eq.start_offset());
            let Some(body) = def.body() else { return };
            if src.line_of(body.location().start_offset()) > eq_line + 1
                && src.lines.get(eq_line).is_some_and(|l| l.is_empty())
            {
                let start = src.line_start(eq_line + 1);
                cx.add(start, start + 1, "Extra empty line detected at method body beginning.");
            }
            return;
        }
        // `node.arguments.source_range&.last_line`: parenthesised or bare parameter list.
        let first_line = def
            .rparen_loc()
            .map(|l| l.end_offset())
            .or_else(|| def.parameters().map(|p| p.location().end_offset()))
            .map(|e| src.line_of(e - 1));
        let loc = node.location();
        let range = (loc.start_offset(), loc.end_offset());
        helpers::empty_lines_around_body::check(cx, "method", "no_empty_lines", range, None, first_line);
    }
}
