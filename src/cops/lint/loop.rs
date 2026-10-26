use crate::prelude::*;

pub const NAME: &str = "Lint/Loop";
pub static COP: Loop = Loop;
pub struct Loop;

impl Cop for Loop {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::WHILE_NODE, kind::UNTIL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let (post, kw, pred, stmts, cond_kw) = match node.as_while_node() {
            Some(w) => (w.is_begin_modifier(), w.keyword_loc(), w.predicate(), w.statements(), "unless"),
            None => {
                let u = node.as_until_node().unwrap();
                (u.is_begin_modifier(), u.keyword_loc(), u.predicate(), u.statements(), "if")
            }
        };
        if !post {
            return;
        }
        let Some(body) = stmts.and_then(|s| s.body().iter().next()).and_then(|b| b.as_begin_node()) else { return };
        let (Some(begin_kw), Some(end_kw)) = (body.begin_keyword_loc(), body.end_keyword_loc()) else { return };
        let loc = node.location();
        let column = helpers::alignment::display_column(cx.src, loc.start_offset());
        let indent = " ".repeat(column);
        let mut break_line = format!("break {cond_kw} {}\n{indent}", node_text(&pred));
        let (bs, be) = (begin_kw.start_offset(), begin_kw.end_offset());
        let (es, ee) = (end_kw.start_offset(), end_kw.end_offset());
        // A single-line `begin ... end` keeps its body and `end` on one line, so the `break` has
        // to start a line of its own, replacing the space before `end` (1.92).
        let single_line = cx.src.line_of(bs) == cx.src.line_of(ee);
        let mut space_start = es;
        if single_line {
            let width = helpers::alignment::configured_indentation_width(cx);
            let inner = " ".repeat((column as i64 + width).max(0) as usize);
            break_line = format!("\n{inner}{break_line}");
            let before = cx.src.text[..es].trim_end_matches([' ', '\t']);
            space_start = before.len();
        }
        let end = loc.end_offset();
        cx.add_fix(
            kw.start_offset(),
            kw.end_offset(),
            "Use `Kernel#loop` with `break` rather than `begin/end/until`(or `while`).",
            |c| {
                c.replace(bs, be, "loop do");
                c.remove(ee, end);
                if single_line {
                    c.replace(space_start, es, &break_line);
                } else {
                    c.insert_before(es, ee, &break_line);
                }
            },
        );
    }
}
