//! Ruby String semantics that differ from Rust's (Unicode-aware) defaults.

/// Ruby's `\s` as a char: ASCII space, \t, \n, \v, \f, \r (not NBSP or U+3000, which Rust's
/// `char::is_whitespace` matches, and including \v, which `is_ascii_whitespace` leaves out).
pub fn is_space(c: char) -> bool {
    c.is_ascii() && crate::helpers::parser_tokens::is_ruby_space(c as u8)
}

fn is_strip(c: char) -> bool {
    is_space(c) || c == '\0'
}

/// String#strip: removes ASCII whitespace and NUL only.
pub fn strip(s: &str) -> &str {
    s.trim_matches(is_strip)
}

/// String#lstrip.
pub fn lstrip(s: &str) -> &str {
    s.trim_start_matches(is_strip)
}

/// String#rstrip.
pub fn rstrip(s: &str) -> &str {
    s.trim_end_matches(is_strip)
}

/// RuboCop's `String#blank?` (`empty? || lstrip.empty?`).
pub fn blank(s: &str) -> bool {
    lstrip(s).is_empty()
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
    // sym_printable: Onigmo's Print class (approximated like `inspect`; it keeps U+0085).
    let printable = |c: char| {
        !((c.is_control() && c != '\u{85}')
            || matches!(c, '\u{2028}' | '\u{2029}' | '\u{fdd0}'..='\u{fdef}')
            || (c as u32 & 0xfffe) == 0xfffe)
    };
    if simple && s.chars().all(printable) {
        format!(":{s}")
    } else if s.is_ascii() {
        // An all-ASCII symbol is US-ASCII: its inspect escapes other controls as `\xNN`.
        format!(":{}", inspect_binary(name))
    } else {
        format!(":{}", inspect(name))
    }
}

/// Ruby's `Regexp.escape`.
pub fn regexp_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            '\x0c' => out.push_str("\\f"),
            '\x0b' => out.push_str("\\v"),
            ' ' => out.push_str("\\ "),
            '.' | '-' | '#' | '[' | ']' | '{' | '}' | '(' | ')' | '|' | '*' | '?' | '+' | '^' | '$' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    out
}

/// Ruby's `Float#to_s` (shortest round-trip digits; fixed notation for exponents in -4..16).
pub fn float_to_s(f: f64) -> String {
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

/// A Ruby regexp source rewritten for the regex crate: Ruby's `\w \W \s \S \d \D \h \H` are
/// ASCII-only classes (Rust's are Unicode), and `\Z` allows a final newline. Works inside
/// character classes too (the regex crate nests classes).
pub fn ruby_regex_classes(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut chars = src.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        let Some(e) = chars.next() else {
            out.push(c);
            break;
        };
        match e {
            'w' => out.push_str("[0-9A-Za-z_]"),
            'W' => out.push_str("[^0-9A-Za-z_]"),
            's' => out.push_str(r"[\t\n\x0B\x0C\r ]"),
            'S' => out.push_str(r"[^\t\n\x0B\x0C\r ]"),
            'd' => out.push_str("[0-9]"),
            'D' => out.push_str("[^0-9]"),
            'h' => out.push_str("[0-9A-Fa-f]"),
            'H' => out.push_str("[^0-9A-Fa-f]"),
            'Z' => out.push_str(r"\n?\z"),
            // Onigmo's Unicode `\p{Word}` / `\P{Word}` are the regex crate's (Unicode) `\w` / `\W`.
            'p' | 'P' if chars.as_str().starts_with("{Word}") => {
                out.push_str(if e == 'p' { r"\w" } else { r"\W" });
                chars.nth("{Word}".len() - 1);
            }
            e => {
                out.push('\\');
                out.push(e);
            }
        }
    }
    out
}
