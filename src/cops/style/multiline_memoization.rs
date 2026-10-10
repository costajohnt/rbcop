use crate::prelude::*;

pub const NAME: &str = "Style/MultilineMemoization";
pub static COP: MultilineMemoization = MultilineMemoization;
pub struct MultilineMemoization;

impl Cop for MultilineMemoization {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[
            kind::LOCAL_VARIABLE_OR_WRITE_NODE,
            kind::INSTANCE_VARIABLE_OR_WRITE_NODE,
            kind::CLASS_VARIABLE_OR_WRITE_NODE,
            kind::GLOBAL_VARIABLE_OR_WRITE_NODE,
            kind::CONSTANT_OR_WRITE_NODE,
            kind::CONSTANT_PATH_OR_WRITE_NODE,
            kind::CALL_OR_WRITE_NODE,
            kind::INDEX_OR_WRITE_NODE,
        ]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let Some((_, rhs)) = assignment_parts(node) else { return };
        let src = cx.src;
        let (rs, re) = (rhs.location().start_offset(), rhs.location().end_offset());
        if src.line_of(rs) == src.line_of(re) {
            return;
        }
        let braces = cx.cfg.str("EnforcedStyle") == Some("braces");
        // (open, close) delimiters to replace.
        let (open, close) = if braces {
            let Some(b) = rhs.as_begin_node() else { return };
            let Some(kw) = b.begin_keyword_loc() else { return };
            if b.rescue_clause().is_some() || b.ensure_clause().is_some() {
                return;
            }
            let Some(end_kw) = b.end_keyword_loc() else { return };
            ((kw.start_offset(), kw.end_offset()), (end_kw.start_offset(), end_kw.end_offset()))
        } else {
            let Some(p) = rhs.as_parentheses_node() else { return };
            let (o, c) = (p.opening_loc(), p.closing_loc());
            ((o.start_offset(), o.end_offset()), (c.start_offset(), c.end_offset()))
        };
        let (open_text, close_text) = if braces {
            ("(".to_string(), ")".to_string())
        } else {
            let col = src.column(rs);
            let width = helpers::alignment::configured_indentation_width(cx) as usize;
            let open_text = if src.text.as_bytes().get(open.1) == Some(&b'\n') {
                "begin".to_string()
            } else {
                format!("begin\n{}", " ".repeat(col + width))
            };
            let line = src.lines.get(src.line_of(close.0) - 1).copied().unwrap_or("");
            let close_text = if line.chars().any(|ch| !ch.is_ascii_whitespace() && ch != ')' && ch != '\x0b') {
                format!("\n{}end", " ".repeat(col))
            } else {
                "end".to_string()
            };
            (open_text, close_text)
        };
        let msg = if braces {
            "Wrap multiline memoization blocks in `(` and `)`."
        } else {
            "Wrap multiline memoization blocks in `begin` and `end`."
        };
        let loc = node.location();
        cx.add_fix(loc.start_offset(), loc.end_offset(), msg, |c| {
            c.replace(open.0, open.1, &open_text);
            c.replace(close.0, close.1, &close_text);
        });
    }
}
