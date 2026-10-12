//! rubocop-performance's RegexpMetacharacter mixin, with RuboCop's Util::LITERAL_REGEX and
//! Util#interpret_string_escapes (StringInterpreter), for the literal-regexp cops.

use crate::prelude::*;

/// One LITERAL_REGEX unit at the start of `s`: `[\w\s\-,"'!#%&<>=;:`~/]` or
/// `\\[^AbBdDgGhHkpPRwWXsSzZ0-9]` (ASCII `\w`/`\s`, as in Ruby). Returns its length in bytes.
fn literal_unit(s: &str) -> Option<usize> {
    let mut it = s.chars();
    let c = it.next()?;
    if c.is_ascii_alphanumeric() || " \t\n\x0b\x0c\r_-,\"'!#%&<>=;:`~/".contains(c) {
        return Some(1);
    }
    if c != '\\' {
        return None;
    }
    let n = it.next()?;
    (!"AbBdDgGhHkpPRwWXsSzZ0123456789".contains(n)).then(|| 1 + n.len_utf8())
}

/// `/\A(?:#{LITERAL_REGEX})+\z/`
pub fn literal(s: &str) -> bool {
    let mut rest = s;
    if rest.is_empty() {
        return false;
    }
    while !rest.is_empty() {
        match literal_unit(rest) {
            Some(n) => rest = &rest[n..],
            None => return false,
        }
    }
    true
}

/// `/\A(?:#{LITERAL_REGEX})+\Z/` (DETERMINISTIC_REGEX): a final newline may follow.
pub fn literal_before_final_newline(s: &str) -> bool {
    literal(s) || s.strip_suffix('\n').is_some_and(literal)
}

/// RegexpMetacharacter#literal_at_start?
pub fn literal_at_start(s: &str, safe_multiline: bool) -> bool {
    s.strip_prefix("\\A").is_some_and(literal) || (!safe_multiline && s.strip_prefix('^').is_some_and(literal))
}

/// RegexpMetacharacter#literal_at_end?
pub fn literal_at_end(s: &str, safe_multiline: bool) -> bool {
    s.strip_suffix("\\z").is_some_and(literal) || (!safe_multiline && s.strip_suffix('$').is_some_and(literal))
}

/// RegexpMetacharacter#drop_start_metacharacter
pub fn drop_start(s: &str) -> &str {
    s.strip_prefix("\\A").unwrap_or_else(|| s.get(1..).unwrap_or(""))
}

/// RegexpMetacharacter#drop_end_metacharacter
pub fn drop_end(s: &str) -> &str {
    s.strip_suffix("\\z").unwrap_or_else(|| s.strip_suffix(|_| true).unwrap_or(""))
}

/// RegexpMetacharacter#safe_multiline?: `cop_config.fetch('SafeMultiline', true)`.
pub fn safe_multiline(cx: &Cx) -> bool {
    match cx.cfg.get("SafeMultiline") {
        None => true,
        Some(Value::Null | Value::Bool(false)) => false,
        Some(_) => true,
    }
}

/// `(regexp (str $_) (regopt))`: the content of a regexp literal with one string part and no
/// options, as the parser gem's `str` child holds it (source text, one line).
pub fn regexp_str<'pr>(node: &Node<'pr>) -> Option<&'pr str> {
    let re = node.as_regular_expression_node()?;
    if re.closing_loc().as_slice().len() != 1 {
        return None;
    }
    let content = text(&re.content_loc());
    // Parser splits a multi-line regexp into one `str` per line; an empty one has none.
    let body = content.strip_suffix('\n').unwrap_or(content);
    (!content.is_empty() && !body.contains('\n')).then_some(content)
}

/// The `redundant_regex?` matcher of StringInclude, StartWith and EndWith: `x.m(/re/)` for
/// `arg_methods`, `/re/.m(x)` for `recv_methods`, and `/re/ =~ x` (a match-with-lvasgn), where the
/// regexp is `(regexp (str $#ok?) (regopt))`. Returns the other operand and the regexp's string.
pub fn redundant_regex<'pr>(
    call: &ruby_prism::CallNode<'pr>,
    arg_methods: &[&str],
    recv_methods: &[&str],
    ok: impl Fn(&str) -> bool,
) -> Option<(Node<'pr>, &'pr str)> {
    let name = method_name(call);
    let recv = call.receiver()?;
    let mut args = send_args(call);
    if args.len() != 1 {
        return None;
    }
    let arg = args.pop().unwrap();
    let matching = |n: &Node<'pr>| regexp_str(n).filter(|s| ok(s));
    let regexp_recv =
        matches!(kind::of(&recv), kind::REGULAR_EXPRESSION_NODE | kind::INTERPOLATED_REGULAR_EXPRESSION_NODE);
    if name == "=~" && !is_csend(call) && regexp_recv {
        return matching(&recv).map(|s| (arg, s));
    }
    if arg_methods.contains(&name)
        && let Some(s) = matching(&arg)
    {
        return Some((recv, s));
    }
    if !is_csend(call) && recv_methods.contains(&name) {
        return matching(&recv).map(|s| (arg, s));
    }
    None
}

/// DeletePrefix / DeleteSuffix: `(call $!nil? ${:gsub :gsub! :sub :sub!} (regexp (str $#anchored?)
/// (regopt)) (str $_))` with an empty replacement becomes `x.delete_prefix('...')`.
pub fn delete_affix(
    cx: &mut Cx,
    node: &Node,
    good: &str,
    anchored: fn(&str, bool) -> bool,
    drop: fn(&str) -> &str,
) {
    let call = node.as_call_node().unwrap();
    let bad = method_name(&call);
    if !matches!(bad, "gsub" | "gsub!" | "sub" | "sub!") {
        return;
    }
    let Some(recv) = call.receiver() else { return };
    let args = send_args(&call);
    let [re, replace] = args.as_slice() else { return };
    let safe = safe_multiline(cx);
    let Some(re_str) = regexp_str(re).filter(|s| anchored(s, safe)) else { return };
    let Some(s) = replace.as_string_node().filter(|_| !is_parser_dstr(replace)) else { return };
    if !s.unescaped().is_empty() {
        return;
    }
    let good = if bad.ends_with('!') { format!("{good}!") } else { good.to_string() };
    let dot = call.call_operator_loc().map_or("", |d| text(&d));
    let value = crate::helpers::percent_array::safe_string_literal(&interpret_string_escapes(drop(re_str)));
    let new_code = format!("{}{dot}{good}({value})", node_text(&recv));
    let sel = call.message_loc().unwrap();
    let (start, end) = send_range(&call);
    cx.add_fix(sel.start_offset(), sel.end_offset(), format!("Use `{good}` instead of `{bad}`."), |c| {
        c.replace(start, end, &new_code)
    });
}

/// Util#interpret_string_escapes (StringInterpreter.interpret).
pub fn interpret_string_escapes(s: &str) -> Vec<u8> {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] != b'\\' || i + 1 >= b.len() {
            out.push(b[i]);
            i += 1;
            continue;
        }
        let c = b[i + 1];
        let simple = match c {
            b'a' => Some(&b"\x07"[..]),
            b'b' => Some(&b"\x08"[..]),
            b'e' => Some(&b"\x1b"[..]),
            b'f' => Some(&b"\x0c"[..]),
            b'n' => Some(&b"\n"[..]),
            b'r' => Some(&b"\r"[..]),
            b's' => Some(&b" "[..]),
            b't' => Some(&b"\t"[..]),
            b'v' => Some(&b"\x0b"[..]),
            b'\n' => Some(&b""[..]),
            _ => None,
        };
        if let Some(r) = simple {
            out.extend_from_slice(r);
            i += 2;
            continue;
        }
        let rest = &b[i + 1..];
        if c.is_ascii_digit() {
            // \d{1,3}: octal byte (to_i(8) stops at the first non-octal digit).
            let n = rest.iter().take(3).take_while(|d| d.is_ascii_digit()).count();
            let v = rest[..n].iter().take_while(|d| (b'0'..=b'7').contains(d)).fold(0u32, |v, d| v * 8 + (d - b'0') as u32);
            out.push(v as u8);
            i += 1 + n;
            continue;
        }
        if c == b'x' && rest.get(1).is_some_and(u8::is_ascii_hexdigit) {
            let n = rest[1..].iter().take(2).take_while(|d| d.is_ascii_hexdigit()).count();
            out.push(u8::from_str_radix(std::str::from_utf8(&rest[1..1 + n]).unwrap(), 16).unwrap());
            i += 2 + n;
            continue;
        }
        if c == b'u' && rest.len() >= 5 && rest[1..5].iter().all(u8::is_ascii_hexdigit) {
            let v = u32::from_str_radix(std::str::from_utf8(&rest[1..5]).unwrap(), 16).unwrap();
            push_char(&mut out, v);
            i += 6;
            continue;
        }
        if c == b'u'
            && rest.get(1) == Some(&b'{')
            && let Some(close) = rest[2..].iter().position(|&d| d == b'}')
        {
            // `escape[3..].split(/\s+/).map(&:hex)`: the closing `}` stays on the last part.
            static WS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[ \t\n\x0b\x0c\r]+").unwrap());
            let raw = String::from_utf8_lossy(&rest[2..2 + close + 1]).into_owned();
            for part in WS.split(&raw) {
                push_char(&mut out, ruby_hex(part));
            }
            i += 3 + close + 1;
            continue;
        }
        // Any other escaped character stands for itself.
        let len = std::str::from_utf8(&b[i + 1..]).ok().and_then(|t| t.chars().next()).map_or(1, char::len_utf8);
        out.extend_from_slice(&b[i + 1..i + 1 + len]);
        i += 1 + len;
    }
    out
}

/// String#hex: leading hex digits, 0 if none.
fn ruby_hex(s: &str) -> u32 {
    let digits: String = s.chars().take_while(char::is_ascii_hexdigit).collect();
    u32::from_str_radix(&digits, 16).unwrap_or(0)
}

fn push_char(out: &mut Vec<u8>, v: u32) {
    let c = char::from_u32(v).unwrap_or('\u{fffd}');
    let mut buf = [0; 4];
    out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literal_units() {
        assert!(literal("abc"));
        assert!(literal("a\\.b"));
        assert!(!literal("a.b"));
        assert!(!literal("\\d"));
        assert!(!literal(""));
        assert!(literal_at_start("\\Aabc", true));
        assert!(!literal_at_start("^abc", true));
        assert!(literal_at_start("^abc", false));
        assert!(literal_at_end("abc\\z", true));
        assert_eq!(drop_end("abc$"), "abc");
        assert_eq!(drop_start("^abc"), "abc");
        assert_eq!(interpret_string_escapes("a\\.b\\n\\x41\\u0042"), b"a.b\nAB");
    }
}
