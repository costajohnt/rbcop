use crate::prelude::*;

pub const NAME: &str = "Layout/EmptyLines";
pub static COP: EmptyLines = EmptyLines;
pub struct EmptyLines;

impl Cop for EmptyLines {
    fn correctable(&self) -> bool {
        true
    }

    fn on_file(&self, cx: &mut Cx) {
        let src = cx.src;
        // RuboCop checks the raw source: a CRLF between the newlines breaks the quick check.
        if !src.raw_text.contains("\n\n\n") || helpers::parser_tokens::tokens_empty(src, cx.facts.first_code) {
            return;
        }
        // Lines with a parser token: lexer tokens (a statement's newline after a `\` continuation
        // sits on a blank line) plus one per line of string/regexp content.
        let mut lines: Vec<usize> =
            helpers::parser_tokens::sorted_tokens(src).iter().map(|t| src.line_of(t.start)).collect();
        lines.extend((1..cx.facts.string_lines.len()).filter(|&l| cx.facts.string_lines[l]));
        lines.sort_unstable();
        lines.dedup();
        // each_extra_empty_line: gaps of more than two lines between token lines, from line 1.
        let empty = |line: usize| src.lines.get(line - 1).is_some_and(|l| l.is_empty());
        let mut prev = 1;
        for cur in lines {
            if cur > prev + 2 {
                for line in prev + 1..cur {
                    if empty(line - 1) && empty(line) {
                        let start = src.line_start(line);
                        cx.add_fix(start, start + 1, "Extra blank line detected.", |c| c.remove(start, start + 1));
                    }
                }
            }
            prev = cur;
        }
    }
}
