//! `Hash#to_yaml` (Psych + libyaml) for config values, as `--show-cops` prints them.

use crate::config::value::{Map, Value};
use regex::Regex;
use std::sync::LazyLock;

/// The mapping as Psych dumps it, without the leading `---` line.
pub fn dump(map: &Map) -> String {
    let mut out = String::new();
    map_body(&mut out, map, 0);
    out
}

fn map_body(out: &mut String, map: &Map, indent: usize) {
    for (k, v) in &map.0 {
        let key = scalar(&Value::Str(k.clone()), indent, indent + 2);
        out.push_str(&" ".repeat(indent));
        out.push_str(&key);
        out.push(':');
        value(out, v, indent, indent + key.chars().count() + 2);
    }
}

/// A value after `key:` (or `-`); `col` is where an inline scalar would start.
fn value(out: &mut String, v: &Value, indent: usize, col: usize) {
    match v {
        Value::Null => out.push('\n'),
        Value::Seq(s) if s.is_empty() => out.push_str(" []\n"),
        Value::Map(m) if m.0.is_empty() => out.push_str(" {}\n"),
        Value::Seq(items) => {
            out.push('\n');
            seq_body(out, items, indent);
        }
        Value::Map(m) => {
            out.push('\n');
            map_body(out, m, indent + 2);
        }
        v => {
            out.push(' ');
            out.push_str(&scalar(v, col, indent + 2));
            out.push('\n');
        }
    }
}

/// Block sequence; in a mapping it is not indented past its key.
fn seq_body(out: &mut String, items: &[Value], indent: usize) {
    for item in items {
        out.push_str(&" ".repeat(indent));
        out.push('-');
        match item {
            Value::Seq(s) if !s.is_empty() => {
                let mut inner = String::new();
                seq_body(&mut inner, s, indent + 2);
                out.push(' ');
                out.push_str(&inner[indent + 2..]);
            }
            Value::Map(m) if !m.0.is_empty() => {
                let mut inner = String::new();
                map_body(&mut inner, m, indent + 2);
                out.push(' ');
                out.push_str(&inner[indent + 2..]);
            }
            v => value(out, v, indent, indent + 2),
        }
    }
}

#[derive(PartialEq)]
enum Style {
    Plain,
    Single,
    Double,
}

fn scalar(v: &Value, col: usize, cont: usize) -> String {
    match v {
        Value::Null => String::new(),
        Value::Bool(b) => b.to_string(),
        Value::Int(i) => i.to_string(),
        Value::Float(f) if f.fract() == 0.0 && f.abs() < 1e16 => format!("{f:.1}"),
        Value::Float(f) => f.to_string(),
        Value::Regex(r) => format!("!ruby/regexp {r}"),
        Value::Str(s) => string(s, col, cont),
        Value::Seq(_) | Value::Map(_) => String::new(),
    }
}

/// Psych::Visitors::YAMLTree#visit_String, then libyaml's choice when Psych leaves it open.
fn string(s: &str, col: usize, cont: usize) -> String {
    static NON_WORD_START: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"^[^\w][^"]*$"#).unwrap());
    static OCTALISH: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\A0[0-7]*[89]").unwrap());
    let style = if s.trim_end_matches('\n').contains('\n') || s.chars().any(|c| c.is_control() && c != '\n') {
        Style::Double
    } else if s == "<<" {
        Style::Single
    } else if matches!(s, "y" | "Y" | "n" | "N") || NON_WORD_START.is_match(s) {
        Style::Double
    } else if !tokenizes_to_string(s) || OCTALISH.is_match(s) || !plain_allowed(s) {
        Style::Single
    } else {
        Style::Plain
    };
    match style {
        Style::Plain => fold(s, col, cont),
        Style::Single => format!("'{}'", fold(&s.replace('\'', "''"), col + 1, cont)),
        Style::Double => {
            let mut out = String::from("\"");
            for c in s.chars() {
                match c {
                    '"' => out.push_str("\\\""),
                    '\\' => out.push_str("\\\\"),
                    '\n' => out.push_str("\\n"),
                    '\t' => out.push_str("\\t"),
                    '\r' => out.push_str("\\r"),
                    c if c.is_control() => out.push_str(&format!("\\x{:02X}", c as u32)),
                    c => out.push(c),
                }
            }
            out.push('"');
            out
        }
    }
}

/// libyaml breaks a plain or single-quoted scalar at a space once the line is past 80 columns.
fn fold(s: &str, col: usize, cont: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::new();
    let mut column = col;
    let mut prev_space = false;
    for (i, &c) in chars.iter().enumerate() {
        if c == ' ' {
            let next_space = chars.get(i + 1) == Some(&' ');
            if !prev_space && column > 80 && !next_space && i != 0 && i + 1 != chars.len() {
                out.push('\n');
                out.push_str(&" ".repeat(cont));
                column = cont;
            } else {
                out.push(c);
                column += 1;
            }
            prev_space = true;
        } else {
            out.push(c);
            column += 1;
            prev_space = false;
        }
    }
    out
}

/// libyaml's analysis: may this be a plain scalar in block context?
fn plain_allowed(s: &str) -> bool {
    !(s.is_empty()
        || s.starts_with(' ')
        || s.ends_with(' ')
        || s.starts_with("---")
        || s.starts_with("...")
        || s.starts_with(['#', ',', '[', ']', '{', '}', '&', '*', '!', '|', '>', '\'', '"', '%', '@', '`'])
        || s.starts_with("- ")
        || s.starts_with("? ")
        || s.starts_with(": ")
        || s == "-"
        || s == "?"
        || s == ":"
        || s.contains(": ")
        || s.ends_with(':')
        || s.contains(" #"))
}

/// Psych::ScalarScanner#tokenize returns a String (rather than nil, a bool, a number, a date...).
fn tokenizes_to_string(s: &str) -> bool {
    static STRINGY: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^[^\d.:-]?[\p{Alphabetic}_\s!@#$%\^&*(){}<>|/\\~;=]+").unwrap());
    static RULES: LazyLock<Vec<Regex>> = LazyLock::new(|| {
        [
            r"^-?\d{4}-\d{1,2}-\d{1,2}(?:[Tt]|\s+)\d{1,2}:\d\d:\d\d(?:\.\d*)?(?:\s*(?:Z|[-+]\d{1,2}:?(?:\d\d)?))?$",
            r"^\d{4}-(?:1[012]|0\d|\d)-(?:[12]\d|3[01]|0\d|\d)$",
            r"(?i)^\+?\.inf$",
            r"(?i)^-\.inf$",
            r"(?i)^\.nan$",
            r"^:.",
            r"^[-+]?[0-9][0-9_]*(:[0-5]?[0-9]){1,2}$",
            r"^[-+]?[0-9][0-9_]*(:[0-5]?[0-9]){1,2}\.[0-9_]*$",
            r"^(?:[-+]?0b[_,]*[0-1][0-1_,]*|[-+]?0[_,]*[0-7][0-7_,]*|[-+]?(?:0|[1-9](?:[0-9]|,[0-9]|_[0-9])*)|[-+]?0x[_,]*[0-9a-fA-F][0-9a-fA-F_,]*)$",
        ]
        .iter()
        .map(|r| Regex::new(r).unwrap())
        .collect()
    });
    static FLOAT: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^(?:[-+]?([0-9][0-9_,]*)?\.[0-9]*([eE][-+][0-9]+)?)$").unwrap());
    if s.is_empty() {
        return false;
    }
    if STRINGY.is_match(s) || s.contains('\n') {
        if s.chars().count() > 5 {
            return true;
        }
        let l = s.to_lowercase();
        return !(s == "~" || matches!(l.as_str(), "null" | "yes" | "true" | "on" | "no" | "false" | "off"));
    }
    if FLOAT.is_match(s) {
        return matches!(s, "." | "+." | "-.");
    }
    !RULES.iter().any(|r| r.is_match(s))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn psych_styles() {
        let m = |pairs: &[(&str, Value)]| Map(pairs.iter().map(|(k, v)| (k.to_string(), v.clone())).collect());
        let s = |v: &str| Value::Str(v.into());
        let map = m(&[
            ("a", Value::Null),
            ("b", s("yes")),
            ("c", s("1.0")),
            ("e", s("foo: bar")),
            ("g", s(" lead")),
            ("h", s("trail ")),
            ("i", s("")),
            ("j", s("~")),
            ("l", Value::Seq(vec![])),
            ("n", s(&format!("{} {}", "x".repeat(30), "y".repeat(60)))),
            ("r", s("- a")),
            ("s", s("a,b")),
            ("w", s("0o17")),
            ("x", s("1_000")),
            ("y", s("y")),
            ("aa", s("2024-01-01")),
            ("ad", s("1e3")),
        ]);
        let expected = "a:\nb: 'yes'\nc: '1.0'\ne: 'foo: bar'\ng: \" lead\"\nh: 'trail '\ni: ''\nj: \"~\"\nl: []\n\"n\": \
            xxxxxxxxxxxxxxxxxxxxxxxxxxxxxx yyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyy\nr: \"- a\"\ns: a,b\n\
            w: 0o17\nx: '1_000'\n\"y\": \"y\"\naa: '2024-01-01'\nad: 1e3\n";
        assert_eq!(dump(&map), expected);
    }
}
