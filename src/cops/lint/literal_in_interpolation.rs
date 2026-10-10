use crate::helpers::ruby_string;
use crate::prelude::*;

pub const NAME: &str = "Lint/LiteralInInterpolation";
pub static COP: LiteralInInterpolation = LiteralInInterpolation;
pub struct LiteralInInterpolation;

const MSG: &str = "Literal interpolation detected.";

/// Parser `str` (a single-line string literal, `?a`, a one-line heredoc, `__FILE__`).
fn parser_str(node: &Node) -> bool {
    (node.as_string_node().is_some() && !is_parser_dstr(node)) || node.as_source_file_node().is_some()
}

fn unescaped(node: &Node) -> Vec<u8> {
    node.as_string_node().map(|s| s.unescaped().to_vec()).unwrap_or_default()
}

/// rubocop-ast `basic_literal?`.
fn basic_literal(node: &Node) -> bool {
    parser_str(node)
        || matches!(
            kind::of(node),
            kind::INTEGER_NODE
                | kind::FLOAT_NODE
                | kind::SYMBOL_NODE
                | kind::TRUE_NODE
                | kind::FALSE_NODE
                | kind::NIL_NODE
                | kind::RATIONAL_NODE
                | kind::IMAGINARY_NODE
                | kind::SOURCE_LINE_NODE
        )
}

/// `prints_as_self?`; Err where RuboCop crashes on a nil child (endless/beginless range).
fn prints_as_self(node: Option<&Node>) -> Result<bool, ()> {
    let node = node.ok_or(())?;
    if basic_literal(node) {
        return Ok(true);
    }
    let children: Vec<Option<Node>> = match kind::of(node) {
        kind::ARRAY_NODE => node.as_array_node().unwrap().elements().iter().map(Some).collect(),
        kind::HASH_NODE => node.as_hash_node().unwrap().elements().iter().map(Some).collect(),
        kind::ASSOC_NODE => {
            let a = node.as_assoc_node().unwrap();
            vec![Some(a.key()), Some(a.value())]
        }
        kind::RANGE_NODE => {
            let r = node.as_range_node().unwrap();
            vec![r.left(), r.right()]
        }
        _ => return Ok(false),
    };
    for child in children {
        if !prints_as_self(child.as_ref())? {
            return Ok(false);
        }
    }
    Ok(true)
}

/// `string.gsub(/[\\"]|#(?=[@{$])/, '\\\\\&')`
fn escape_string_content(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' || c == '"' || (c == '#' && matches!(chars.peek(), Some('@' | '{' | '$'))) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

fn quote_escaped(s: &str) -> String {
    s.replace('"', "\\\"")
}

/// Integer#to_s of an IntegerNode's value.
fn integer_to_s(node: &ruby_prism::IntegerNode) -> String {
    let value = node.value();
    let (negative, words) = value.to_u32_digits();
    let mut words: Vec<u32> = words.to_vec();
    let mut chunks = Vec::new();
    while words.iter().any(|&w| w != 0) {
        let mut rem: u64 = 0;
        for w in words.iter_mut().rev() {
            let cur = (rem << 32) | u64::from(*w);
            *w = (cur / 1_000_000_000) as u32;
            rem = cur % 1_000_000_000;
        }
        chunks.push(rem as u32);
    }
    let mut out = String::from(if negative { "-" } else { "" });
    match chunks.split_last() {
        None => out.push('0'),
        Some((first, rest)) => {
            out.push_str(&first.to_string());
            for c in rest.iter().rev() {
                out.push_str(&format!("{c:09}"));
            }
        }
    }
    out
}

/// Float#to_s.
fn float_to_s(f: f64) -> String {
    if f.is_nan() {
        return "NaN".into();
    }
    if f.is_infinite() {
        return if f > 0.0 { "Infinity".into() } else { "-Infinity".into() };
    }
    let sign = if f.is_sign_negative() { "-" } else { "" };
    if f == 0.0 {
        return format!("{sign}0.0");
    }
    let e = format!("{:e}", f.abs());
    let (mantissa, exp) = e.split_once('e').unwrap();
    let digits: String = mantissa.chars().filter(|&c| c != '.').collect();
    let decpt = exp.parse::<i64>().unwrap() + 1;
    let body = if decpt > 0 && decpt < 16 {
        let d = decpt as usize;
        if digits.len() <= d {
            format!("{digits}{}.0", "0".repeat(d - digits.len()))
        } else {
            format!("{}.{}", &digits[..d], &digits[d..])
        }
    } else if decpt <= 0 && decpt > -4 {
        format!("0.{}{digits}", "0".repeat((-decpt) as usize))
    } else {
        let rest = if digits.len() > 1 { &digits[1..] } else { "0" };
        let x = decpt - 1;
        format!("{}.{rest}e{}{:02}", &digits[..1], if x < 0 { '-' } else { '+' }, x.abs())
    };
    format!("{sign}{body}")
}

/// Symbol#inspect.
fn symbol_inspect(name: &[u8]) -> String {
    let Ok(s) = std::str::from_utf8(name) else { return format!(":{}", ruby_string::inspect(name)) };
    let ident_start = |c: char| c.is_alphabetic() || c == '_' || !c.is_ascii();
    let ident_char = |c: char| c.is_alphanumeric() || c == '_' || !c.is_ascii();
    let ident = |t: &str| t.chars().next().is_some_and(ident_start) && t.chars().all(ident_char);
    const OPS: [&str; 30] = [
        "[]", "[]=", "!", "!=", "!~", "=~", "==", "===", "<=>", "<", "<=", ">", ">=", "<<", ">>", "+", "-", "*", "/",
        "%", "**", "~", "+@", "-@", "!@", "~@", "&", "|", "^", "`",
    ];
    let simple = OPS.contains(&s)
        || s.strip_prefix("@@").is_some_and(ident)
        || s.strip_prefix('@').is_some_and(ident)
        || s.strip_prefix('$').is_some_and(|g| {
            ident(g)
                || (!g.is_empty() && g.bytes().all(|b| b.is_ascii_digit()))
                || (g.len() == 1 && "~*$?!@/\\;,.=:<>\"&`'+0".contains(g))
                || (g.len() == 2 && g.starts_with('-') && g[1..].chars().all(ident_char))
        })
        || {
            let base = s.strip_suffix(['?', '!', '=']).unwrap_or(s);
            ident(base)
        };
    if simple { format!(":{s}") } else { format!(":{}", ruby_string::inspect(name)) }
}

struct Values<'a> {
    text: &'a str,
}

impl Values<'_> {
    fn source(&self, node: &Node) -> String {
        quote_escaped(node_text(node))
    }

    /// `autocorrected_value` / `autocorrected_value_in_hash`.
    fn value(&self, node: &Node, in_hash: bool) -> String {
        match kind::of(node) {
            kind::INTEGER_NODE => integer_to_s(&node.as_integer_node().unwrap()),
            kind::FLOAT_NODE => float_to_s(node.as_float_node().unwrap().value()),
            kind::STRING_NODE if parser_str(node) => {
                let bytes = unescaped(node);
                match (in_hash, std::str::from_utf8(&bytes)) {
                    (true, _) => escape_string_content(&ruby_string::inspect(&bytes)),
                    (false, Ok(s)) => escape_string_content(s),
                    (false, Err(_)) => strip_quotes(node_text(node)).to_string(),
                }
            }
            kind::SYMBOL_NODE => {
                let sym = node.as_symbol_node().unwrap();
                if in_hash {
                    return escape_string_content(&symbol_inspect(sym.unescaped()));
                }
                let l = node.location();
                let start = sym.opening_loc().map_or(l.start_offset(), |o| o.end_offset());
                let end = sym.closing_loc().map_or(l.end_offset(), |c| c.start_offset());
                quote_escaped(&self.text[start..end])
            }
            kind::ARRAY_NODE => {
                let array = node.as_array_node().unwrap();
                match (array.opening_loc(), array.closing_loc()) {
                    (Some(o), Some(c)) if o.as_slice().starts_with(b"%") => {
                        let contents = &self.text[o.end_offset()..c.start_offset()];
                        let words: Vec<String> = contents
                            .split([' ', '\t', '\n', '\x0b', '\x0c', '\r'])
                            .filter(|w| !w.is_empty())
                            .map(|w| ruby_string::inspect(w.as_bytes()))
                            .collect();
                        quote_escaped(&format!("[{}]", words.join(", ")))
                    }
                    _ => self.source(node),
                }
            }
            kind::HASH_NODE => {
                let pairs: Vec<String> = node
                    .as_hash_node()
                    .unwrap()
                    .elements()
                    .iter()
                    .map(|p| {
                        let a = p.as_assoc_node().unwrap();
                        format!("{}=>{}", self.value(&a.key(), true), self.value(&a.value(), true))
                    })
                    .collect();
                format!("{{{}}}", pairs.join(", "))
            }
            kind::NIL_NODE if !in_hash => String::new(),
            _ => self.source(node),
        }
    }
}

/// `source.delete_prefix('"').delete_suffix('"')`
fn strip_quotes(s: &str) -> &str {
    let s = s.strip_prefix('"').unwrap_or(s);
    s.strip_suffix('"').unwrap_or(s)
}

/// handle_special_regexp_chars: `(\\*)/` keeps the compiled number of backslashes.
fn escape_slashes(value: &str) -> String {
    let mut out = String::new();
    let mut backslashes = 0;
    for c in value.chars() {
        match c {
            '\\' => backslashes += 1,
            '/' => {
                let needed = 2 * ((backslashes + 1) / 4) + 1;
                out.push_str(&"\\".repeat(needed));
                out.push('/');
                backslashes = 0;
            }
            _ => {
                out.push_str(&"\\".repeat(backslashes));
                backslashes = 0;
                out.push(c);
            }
        }
    }
    out.push_str(&"\\".repeat(backslashes));
    out
}

fn is_regexp(node: &Node) -> bool {
    matches!(kind::of(node), kind::INTERPOLATED_REGULAR_EXPRESSION_NODE | kind::INTERPOLATED_MATCH_LAST_LINE_NODE)
}

impl Cop for LiteralInInterpolation {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::EMBEDDED_STATEMENTS_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let e = node.as_embedded_statements_node().unwrap();
        let Some(last) = e.statements().and_then(|s| s.body().iter().last()) else { return };
        let Some(parent) = cx.parent() else { return };
        let src = cx.src;
        let text = node_text(&last);
        // special_keyword?
        let no_begin = last.as_string_node().is_some_and(|s| s.opening_loc().is_none() || is_heredoc(&last));
        if (parser_str(&last) && (no_begin || last.as_source_file_node().is_some())) || text == "__LINE__" {
            return;
        }
        if prints_as_self(Some(&last)) != Ok(true) {
            return;
        }
        let l = last.location();
        let (start, end) = (l.start_offset(), l.end_offset());
        // Special case for Layout/TrailingWhitespace: a blank string ending a heredoc line.
        let space = parser_str(&last) && {
            std::str::from_utf8(&unescaped(&last)).is_ok_and(|s| s.trim_start_matches([' ', '\t', '\n', '\x0b', '\x0c', '\r', '\0']).is_empty())
        };
        if space && parent.as_interpolated_string_node().is_some() && is_heredoc(parent) {
            let line = src.lines[src.line_of(end) - 1];
            if line.chars().count() == src.column(end) + 1 {
                return;
            }
        }
        if last.as_array_node().is_some() && is_regexp(parent) {
            return;
        }
        let values = Values { text: src.text };
        let mut value = values.value(&last, false);
        let slash_regexp = parent
            .as_interpolated_regular_expression_node()
            .is_some_and(|r| r.opening_loc().as_slice() == b"/")
            || parent.as_interpolated_match_last_line_node().is_some_and(|r| r.opening_loc().as_slice() == b"/");
        if slash_regexp && value.contains('/') {
            value = escape_slashes(&value);
        }
        // in_array_percent_literal?
        let n = cx.ancestors.len();
        let in_percent_array = matches!(kind::of(parent), kind::INTERPOLATED_STRING_NODE | kind::INTERPOLATED_SYMBOL_NODE)
            && n >= 2
            && cx.ancestors[n - 2]
                .as_array_node()
                .is_some_and(|a| a.opening_loc().is_some_and(|o| o.as_slice().starts_with(b"%")));
        if in_percent_array && (value.is_empty() || value.contains([' ', '\t', '\n', '\x0b', '\x0c', '\r'])) {
            return;
        }
        let invalid_str =
            parser_str(&last) && std::str::from_utf8(&unescaped(&last)).is_err();
        let replacement = if invalid_str { strip_quotes(text).to_string() } else { value };
        let b = node.location();
        let (bs, be) = (b.start_offset(), b.end_offset());
        cx.add_fix(start, end, MSG, |c| c.replace(bs, be, &replacement));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ruby_float_to_s() {
        for (f, s) in [
            (1e20, "1.0e+20"),
            (1e15, "1.0e+15"),
            (1e14, "100000000000000.0"),
            (0.0001, "0.0001"),
            (0.00001, "1.0e-05"),
            (1.5e-7, "1.5e-07"),
            (1.234567890123456e15, "1.234567890123456e+15"),
            (-0.0, "-0.0"),
            (0.0012, "0.0012"),
            (100.0, "100.0"),
            (1.5, "1.5"),
        ] {
            assert_eq!(float_to_s(f), s);
        }
    }

    #[test]
    fn slashes() {
        assert_eq!(escape_slashes("a/b"), "a\\/b");
        assert_eq!(escape_slashes("a\\\\\\/b"), "a\\\\\\/b");
    }
}
