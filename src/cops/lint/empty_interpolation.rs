use crate::prelude::*;

pub const NAME: &str = "Lint/EmptyInterpolation";
pub static COP: EmptyInterpolation = EmptyInterpolation;
pub struct EmptyInterpolation;

impl Cop for EmptyInterpolation {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::EMBEDDED_STATEMENTS_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // in_percent_literal_array?: the nearest array ancestor is a %w/%W/%i/%I literal.
        let array = cx.ancestors.iter().rev().find_map(|a| a.as_array_node());
        if array.is_some_and(|a| a.opening_loc().is_some_and(|o| o.as_slice().starts_with(b"%"))) {
            return;
        }
        let e = node.as_embedded_statements_node().unwrap();
        // `nil_type? || (basic_literal? && str_content&.empty?)`: an empty heredoc or percent
        // literal is a parser `dstr`, not a basic literal, so it keeps the interpolation (#303).
        let empty = e.statements().is_none_or(|s| {
            s.body().iter().all(|c| {
                c.as_nil_node().is_some()
                    || (!is_parser_dstr(&c) && c.as_string_node().is_some_and(|s| s.unescaped().is_empty()))
            })
        });
        if !empty {
            return;
        }
        let l = node.location();
        let (start, end) = (l.start_offset(), l.end_offset());
        cx.add_fix(start, end, "Empty interpolation detected.", |c| c.remove(start, end));
    }
}
