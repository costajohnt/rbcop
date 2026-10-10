//! RuboCop's PercentArray mixin, PercentLiteralCorrector and the Util string helpers they use
//! (shared by Style/SymbolArray and Style/WordArray).

use crate::cop::Cx;
use crate::helpers::ruby_string::inspect;
use crate::source::Source;
use ruby_prism::{ArrayNode, Node};

fn range(node: &Node) -> (usize, usize) {
    let l = node.location();
    (l.start_offset(), l.end_offset())
}

/// Util#double_quotes_required?: a `'`, or an odd run of backslashes not followed by `"`.
fn double_quotes_required(s: &str) -> bool {
    if s.contains('\'') {
        return true;
    }
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\\' {
            let start = i;
            while i < b.len() && b[i] == b'\\' {
                i += 1;
            }
            if (i - start) % 2 == 1 && b.get(i) != Some(&b'"') {
                return true;
            }
        } else {
            i += 1;
        }
    }
    false
}

/// Util#escape_string: `string.inspect[1..-2]` with `\"` unescaped.
pub fn escape_string(s: &[u8]) -> String {
    let i = inspect(s);
    i[1..i.len() - 1].replace("\\\"", "\"")
}

/// Util#needs_escaping?
pub fn needs_escaping(s: &[u8]) -> bool {
    double_quotes_required(&escape_string(s))
}

/// Util#to_string_literal.
pub fn to_string_literal(s: &[u8]) -> String {
    if needs_escaping(s) && std::str::from_utf8(s).is_ok() {
        inspect(s)
    } else {
        let s = String::from_utf8_lossy(s);
        format!("'{}'", s.replace('\\', "\\\\").replace("\\\"", "\""))
    }
}

/// Util#trim_string_interpolation_escape_character: `\#{x}` -> `#{x}` (lazy up to the first `}`).
pub fn trim_string_interpolation_escape_character(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(i) = rest.find("\\#{") {
        let after = &rest[i + 3..];
        // `.` does not match a newline.
        match after.find('}').filter(|&j| !after[..j].contains('\n')) {
            Some(j) => {
                out.push_str(&rest[..i]);
                out.push_str("#{");
                out.push_str(&after[..j]);
                out.push('}');
                rest = &after[j + 1..];
            }
            None => {
                out.push_str(&rest[..i + 1]);
                rest = &rest[i + 1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// PercentArray#invalid_percent_array_context?: an argument of an unparenthesized call that
/// has a literal block (`foo %i[a b] { }`).
pub fn invalid_percent_array_context(ancestors: &[Node]) -> bool {
    let n = ancestors.len();
    if n < 2 || ancestors[n - 1].as_arguments_node().is_none() {
        return false;
    }
    ancestors[n - 2].as_call_node().is_some_and(|c| {
        !crate::ast::is_csend(&c) && c.opening_loc().is_none() && c.block().is_some_and(|b| b.as_block_node().is_some())
    })
}

/// PercentArray#comments_in_array?: comments on lines first_line...last_line.
pub fn comments_in_array(src: &Source, node: &Node) -> bool {
    let (s, e) = range(node);
    let (first, last) = (src.line_of(s), src.line_of(e));
    src.comments.iter().any(|c| (first..last).contains(&src.line_of(c.start)))
}

/// PercentArray#allowed_bracket_array?
pub fn allowed_bracket_array(cx: &Cx, node: &Node) -> bool {
    let size = node.as_array_node().unwrap().elements().iter().count() as i64;
    comments_in_array(cx.src, node)
        || size < cx.cfg.int("MinSize").unwrap_or(0)
        || invalid_percent_array_context(cx.ancestors)
}

/// PercentArray#build_message_for_bracketed_array.
pub fn bracketed_message(code: &str, what: &str) -> String {
    let prefer = if code.contains('\n') { "an array literal `[...]`".to_string() } else { format!("`{code}`") };
    format!("Use {prefer} for an array of {what}.")
}

/// PercentArray#build_bracketed_array_with_appropriate_whitespace.
pub fn bracketed_with_whitespace(src: &Source, node: &ArrayNode, elements: &[String]) -> String {
    let els: Vec<Node> = node.elements().iter().collect();
    let text = src.text;
    let open_end = node.opening_loc().map_or(0, |l| l.end_offset());
    let close_start = node.closing_loc().map_or(0, |l| l.start_offset());
    let leading = &text[open_end..range(&els[0]).0];
    let between = if els.len() >= 2 { &text[range(&els[0]).1..range(&els[1]).0] } else { " " };
    let trailing = &text[range(els.last().unwrap()).1..close_start];
    format!("[{leading}{}{trailing}]", elements.join(&format!(",{between}")))
}

/// PreferredDelimiters for a percent literal type (`%w`, `%I`, ...).
fn delimiters(cx: &Cx, ty: &str) -> (String, String) {
    let cfg = cx.config.for_cop("Style/PercentLiteralDelimiters");
    let pref = cfg.map("PreferredDelimiters");
    let d = pref.and_then(|m| if m.contains("default") { m.str(ty).or_else(|| m.str("default")) } else { m.str(ty) });
    let mut chars = d.unwrap_or("").chars();
    let open = chars.next().map(String::from).unwrap_or_default();
    let close = chars.next().map(String::from).unwrap_or_default();
    (open, close)
}

/// PercentLiteralCorrector#correct as a precomputed replacement for the array node:
/// `values` holds each element's value (`children[0]`).
pub fn percent_literal_replacement(cx: &Cx, node: &Node, values: &[Vec<u8>], char: char) -> String {
    let escape = values.iter().any(|v| needs_escaping(v));
    let char = if escape { char.to_ascii_uppercase() } else { char };
    let (d0, d1) = delimiters(cx, &format!("%{char}"));
    let fix = |v: &[u8]| -> String {
        let content = if escape { escape_string(v) } else { String::from_utf8_lossy(v).into_owned() };
        if d0 != d1 && content.matches(d0.as_str()).count() == content.matches(d1.as_str()).count() {
            return content;
        }
        let mut content = content;
        for d in [&d0, &d1] {
            if !d.is_empty() {
                content = content.replace(d.as_str(), &format!("\\{d}"));
            }
        }
        content
    };
    let src = cx.src;
    let (s, e) = range(node);
    let source = &src.text[s..e];
    let els: Vec<Node> = node.as_array_node().unwrap().elements().iter().collect();
    let contents = if src.line_of(s) == src.line_of(e) {
        els.iter().zip(values).map(|(_, v)| fix(v)).collect::<Vec<_>>().join(" ")
    } else {
        let lines: Vec<&str> = ruby_split_lines(source);
        let base = src.line_of(s);
        let mut prev = base;
        let mut out = String::new();
        for (i, (el, v)) in els.iter().zip(values).enumerate() {
            let (es, ee) = range(el);
            let first = src.line_of(es);
            let breaks = if first == prev {
                if i == 0 && first == base { String::new() } else { " ".to_string() }
            } else {
                let (b, en) = (prev - base + 1, first - base + 1);
                let joined = lines.get(b.min(lines.len())..en.min(lines.len())).unwrap_or_default().join("\n");
                let word = &src.text[es..ee];
                let before = joined.split(word).next().unwrap_or("");
                format!("\n{before}")
            };
            prev = src.line_of(ee);
            out.push_str(&breaks);
            out.push_str(&fix(v));
        }
        if let Some(last) = lines.last() {
            let ws = last.len() - last.trim_start_matches(|c: char| c.is_ascii_whitespace()).len();
            if last[ws..].starts_with(']') {
                out.push('\n');
                out.push_str(&last[..ws]);
            }
        }
        out
    };
    format!("%{char}{d0}{contents}{d1}")
}

/// Ruby `str.split("\n")`: trailing empty fields dropped.
fn ruby_split_lines(s: &str) -> Vec<&str> {
    let mut v: Vec<&str> = s.split('\n').collect();
    while v.last() == Some(&"") {
        v.pop();
    }
    v
}

