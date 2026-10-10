use crate::prelude::*;

pub const NAME: &str = "Lint/TripleQuotes";
pub static COP: TripleQuotes = TripleQuotes;
pub struct TripleQuotes;

impl Cop for TripleQuotes {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::INTERPOLATED_STRING_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let dstr = node.as_interpolated_string_node().unwrap();
        let parts: Vec<Node> = dstr.parts().iter().collect();
        let mut empty: Vec<(usize, usize)> = parts
            .iter()
            .filter(|p| p.as_string_node().is_some_and(|s| s.unescaped().is_empty()) && !is_parser_dstr(p))
            .map(|p| (p.location().start_offset(), p.location().end_offset()))
            .collect();
        if empty.is_empty() {
            return;
        }
        if node_text(node).bytes().take_while(|b| matches!(b, b'\'' | b'"')).count() < 3 {
            return;
        }
        if empty.len() == parts.len() {
            empty.remove(0);
        }
        let l = node.location();
        cx.add_fix(
            l.start_offset(),
            l.end_offset(),
            "Delimiting a string with multiple quotes has no effect, use a single quote instead.",
            |c| {
                for (s, e) in empty {
                    c.remove(s, e);
                }
            },
        );
    }
}
