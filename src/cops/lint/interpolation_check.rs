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

/// `interpolated_source`: the string with double quotes, as `%{}` when it contains a `"`, or with
/// its `"` escaped when it continues another string part (a `%` there is the `%` operator).
fn interpolated_source(src: &str, continued: bool) -> String {
    let content = &src[1..src.len() - 1];
    if !content.contains('"') {
        return format!("\"{content}\"");
    }
    if !continued {
        return format!("%{{{content}}}");
    }
    // gsub(/\\.|"/) { |m| m == '"' ? '\"' : m }
    let mut out = String::from("\"");
    let mut chars = content.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => {
                // `\\.` keeps an escape pair as it is (a newline after it is kept anyway).
                out.push(c);
                out.extend(chars.next());
            }
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

/// `valid_syntax?`: the interpolated version parses (as the target's parser does) to a single `dstr`.
fn valid_syntax(dq: &str, version: &'static str, legacy: Option<f64>) -> bool {
    let mut buf = Vec::new();
    let Some(result) = helpers::reparsed_equivalence::parse_clean(dq, &mut buf, version, legacy) else {
        return false;
    };
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
        // single_quoted?
        let (Some(open), Some(close)) = (s.opening_loc(), s.closing_loc()) else { return };
        if open.as_slice() != b"'" || close.as_slice().is_empty() {
            return;
        }
        let src = node_text(node);
        if is_heredoc(node) || cx.ancestors.iter().any(is_heredoc) {
            return;
        }
        if cx.parent().is_some_and(|p| {
            p.as_regular_expression_node().is_some() || p.as_interpolated_regular_expression_node().is_some()
        }) {
            return;
        }
        if !has_interpolation(src) {
            return;
        }
        // continued_string_part?: a part of a `dstr` after another part.
        let start = node.location().start_offset();
        let continued = cx.parent().and_then(|p| p.as_interpolated_string_node()).is_some_and(|d| {
            d.parts().iter().next().is_some_and(|first| first.location().start_offset() != start)
        });
        let replacement = interpolated_source(src, continued);
        if !valid_syntax(&replacement, cx.src.version, cx.src.legacy()) {
            return;
        }
        let end = node.location().end_offset();
        cx.add_fix(start, end, "Interpolation in single quoted string detected. Use double quoted strings if you need interpolation.", |c| {
            c.replace(start, end, &replacement);
        });
    }
}
