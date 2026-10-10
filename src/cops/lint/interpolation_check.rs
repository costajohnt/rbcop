use crate::prelude::*;

pub const NAME: &str = "Lint/InterpolationCheck";
pub static COP: InterpolationCheck = InterpolationCheck;
pub struct InterpolationCheck;

/// `/(?<!\\)#\{.*\}/`: a `#{` not preceded by a backslash, with a `}` later on the same line.
fn has_interpolation(src: &str) -> bool {
    let b = src.as_bytes();
    (0..b.len().saturating_sub(1)).any(|i| {
        b[i] == b'#'
            && b[i + 1] == b'{'
            && (i == 0 || b[i - 1] != b'\\')
            && b[i + 2..].iter().take_while(|&&c| c != b'\n').any(|&c| c == b'}')
    })
}

/// `valid_syntax?`: the double-quoted version parses to a single `dstr`.
fn valid_syntax(src: &str) -> bool {
    let dq = if src.contains('"') {
        let s = src.strip_prefix('\'').map_or(src.to_string(), |s| format!("%{{{s}"));
        match s.strip_suffix('\'') {
            Some(s) => format!("{s}}}"),
            None => s,
        }
    } else {
        let s = src.strip_prefix('\'').map_or(src.to_string(), |s| format!("\"{s}"));
        match s.strip_suffix('\'') {
            Some(s) => format!("{s}\""),
            None => s,
        }
    };
    let result = ruby_prism::parse(dq.as_bytes());
    if result.errors().next().is_some() {
        return false;
    }
    let program = result.node();
    let stmts: Vec<Node> = program.as_program_node().unwrap().statements().body().iter().collect();
    stmts.len() == 1 && stmts[0].as_interpolated_string_node().is_some()
}

impl Cop for InterpolationCheck {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::STRING_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let s = node.as_string_node().unwrap();
        let (Some(open), Some(close)) = (s.opening_loc(), s.closing_loc()) else { return };
        let src = node_text(node);
        // A string spanning lines is a `dstr` in parser, checked only when single-quoted.
        if src.contains('\n') && open.as_slice() != b"'" {
            return;
        }
        if is_heredoc(node) || cx.ancestors.iter().any(is_heredoc) {
            return;
        }
        if cx.parent().is_some_and(|p| {
            p.as_regular_expression_node().is_some() || p.as_interpolated_regular_expression_node().is_some()
        }) {
            return;
        }
        if close.as_slice().is_empty() || !has_interpolation(src) || !valid_syntax(src) {
            return;
        }
        let (start, end) = if src.contains('"') { ("%{", "}") } else { ("\"", "\"") };
        let (os, oe, cs, ce) = (open.start_offset(), open.end_offset(), close.start_offset(), close.end_offset());
        let l = node.location();
        cx.add_fix(l.start_offset(), l.end_offset(), "Interpolation in single quoted string detected. Use double quoted strings if you need interpolation.", |c| {
            c.replace(os, oe, start);
            c.replace(cs, ce, end);
        });
    }
}
