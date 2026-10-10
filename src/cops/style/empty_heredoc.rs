use crate::prelude::*;

pub const NAME: &str = "Style/EmptyHeredoc";
pub static COP: EmptyHeredoc = EmptyHeredoc;
pub struct EmptyHeredoc;

impl Cop for EmptyHeredoc {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::STRING_NODE, kind::INTERPOLATED_STRING_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let Some(((start, end), (end_start, end_end), true)) = helpers::heredoc::parts(node) else { return };
        let double = cx.config.for_cop("Style/StringLiterals").str("EnforcedStyle") == Some("double_quotes");
        let literal = if double { "\"\"" } else { "''" };
        let text = cx.src.text;
        // An empty heredoc_body starts and ends where the terminator line starts.
        let body = helpers::whole_lines::range_by_whole_lines(text, end_start, end_start, true);
        let term = helpers::whole_lines::range_by_whole_lines(text, end_start, end_end, true);
        cx.add_fix(start, end, "Use an empty string literal instead of heredoc.", |c| {
            c.replace(start, end, literal);
            c.remove(body.0, body.1);
            c.remove(term.0, term.1);
        });
    }
}
