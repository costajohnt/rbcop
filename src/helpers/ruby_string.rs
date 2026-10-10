//! Ruby String semantics that differ from Rust's (Unicode-aware) defaults.

/// String#strip: removes ASCII whitespace and NUL only.
pub fn strip(s: &str) -> &str {
    s.trim_matches(|c: char| matches!(c, ' ' | '\t' | '\n' | '\x0b' | '\x0c' | '\r' | '\0'))
}

/// String#inspect of a UTF-8 string (invalid bytes as `\xNN`).
// ponytail: "non-printable" approximated as controls, U+2028/9 and noncharacters (no Unicode tables).
pub fn inspect(bytes: &[u8]) -> String {
    let mut out = String::from("\"");
    for chunk in bytes.utf8_chunks() {
        let mut chars = chunk.valid().chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                '"' => out.push_str("\\\""),
                '\\' => out.push_str("\\\\"),
                '\n' => out.push_str("\\n"),
                '\t' => out.push_str("\\t"),
                '\r' => out.push_str("\\r"),
                '\x0c' => out.push_str("\\f"),
                '\x0b' => out.push_str("\\v"),
                '\x08' => out.push_str("\\b"),
                '\x07' => out.push_str("\\a"),
                '\x1b' => out.push_str("\\e"),
                '#' if matches!(chars.peek(), Some('{' | '$' | '@')) => out.push_str("\\#"),
                c if c.is_control()
                    || matches!(c, '\u{2028}' | '\u{2029}' | '\u{fdd0}'..='\u{fdef}')
                    || (c as u32 & 0xfffe) == 0xfffe =>
                {
                    if (c as u32) < 0x10000 {
                        out.push_str(&format!("\\u{:04X}", c as u32));
                    } else {
                        out.push_str(&format!("\\u{{{:X}}}", c as u32));
                    }
                }
                c => out.push(c),
            }
        }
        for b in chunk.invalid() {
            out.push_str(&format!("\\x{b:02X}"));
        }
    }
    out.push('"');
    out
}

/// String#inspect of an ASCII-8BIT (binary) string: bytes >= 0x80 and other non-printables as `\xNN`.
pub fn inspect_binary(bytes: &[u8]) -> String {
    let mut out = String::from("\"");
    for (i, &b) in bytes.iter().enumerate() {
        match b {
            b'"' => out.push_str("\\\""),
            b'\\' => out.push_str("\\\\"),
            b'\n' => out.push_str("\\n"),
            b'\t' => out.push_str("\\t"),
            b'\r' => out.push_str("\\r"),
            0x0c => out.push_str("\\f"),
            0x0b => out.push_str("\\v"),
            0x08 => out.push_str("\\b"),
            0x07 => out.push_str("\\a"),
            0x1b => out.push_str("\\e"),
            b'#' if matches!(bytes.get(i + 1), Some(b'{' | b'$' | b'@')) => out.push_str("\\#"),
            0x20..=0x7e => out.push(b as char),
            _ => out.push_str(&format!("\\x{b:02X}")),
        }
    }
    out.push('"');
    out
}

/// Symbol#inspect (`:a`, `:"a-b"`, `:[]=`, `:@x`).
pub fn symbol_inspect(name: &[u8]) -> String {
    let Ok(s) = std::str::from_utf8(name) else { return format!(":{}", inspect(name)) };
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
    if simple { format!(":{s}") } else { format!(":{}", inspect(name)) }
}
