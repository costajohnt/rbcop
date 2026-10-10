use crate::helpers::range_help::{Side, Surrounding, range_with_surrounding_space};
use crate::helpers::statement_modifier::max_line_length;
use crate::helpers::whole_lines::range_by_whole_lines;
use crate::prelude::*;

pub const NAME: &str = "Style/MultilineMethodSignature";
pub static COP: MultilineMethodSignature = MultilineMethodSignature;
pub struct MultilineMethodSignature;

// Ruby `\s` and `String#strip` (strip also drops NUL)
fn is_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\x0b' | '\x0c' | '\r')
}

impl Cop for MultilineMethodSignature {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::DEF_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let def = node.as_def_node().unwrap();
        let Some(params) = def.parameters() else { return };
        let args = parameter_nodes(&params);
        if args.is_empty() {
            return;
        }
        let src = cx.src;
        let start = node.location().start_offset();
        // arguments.last_line: the `)` line, or the last argument's
        let args_end = def.rparen_loc().map_or(params.location().end_offset(), |r| r.end_offset());
        let (opening_line, closing_line) = (src.line_of(start), src.line_of(args_end));
        if opening_line == closing_line {
            return;
        }
        if let Some(max) = max_line_length(cx) {
            let line = src.lines.get(opening_line - 1).copied().unwrap_or("");
            let indentation = line.chars().take_while(|&c| is_space(c)).count();
            let mut width = 0;
            let mut in_space = false;
            for c in src.slice(start, args_end).chars() {
                if !(in_space && is_space(c)) {
                    width += 1;
                }
                in_space = is_space(c);
            }
            if indentation + width > max {
                return;
            }
        }
        let Some(lparen) = def.lparen_loc() else { return };
        let Some(rparen) = def.rparen_loc() else { return };
        let mut joined = args.iter().map(node_text).collect::<Vec<_>>().join(", ");
        let last_line = src.lines.get(closing_line - 1).copied().unwrap_or("");
        let last_line = last_line.trim_matches(|c| is_space(c) || c == '\0');
        let remove_close = last_line.starts_with(')').then(|| {
            joined.push_str(last_line);
            range_by_whole_lines(src.text, rparen.start_offset(), rparen.end_offset(), true)
        });
        let (first, last) = (args[0].location().start_offset(), args[args.len() - 1].location().end_offset());
        let range = range_with_surrounding_space(src.text, first, last, Surrounding { side: Side::Left, ..Default::default() });
        let keyword_end = def.def_keyword_loc().end_offset();
        let prefix = (src.line_of(range.0) != opening_line).then(|| {
            let p = src.slice(keyword_end, lparen.start_offset()).trim_matches(|c| is_space(c) || c == '\0');
            format!(" {p}")
        });
        let (lp0, lp1) = (lparen.start_offset(), lparen.end_offset());
        cx.add_fix(start, node.location().end_offset(), "Avoid multi-line method signatures.", |c| {
            if let Some((a, b)) = remove_close {
                c.remove(a, b);
            }
            if let Some(p) = &prefix {
                c.replace(keyword_end, lp0, p);
            }
            c.remove(range.0, range.1);
            c.insert_after(lp0, lp1, &joined);
        });
    }
}
