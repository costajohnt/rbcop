//! `Hash#to_yaml` (Psych + libyaml) for config values, as `--show-cops` prints them: Psych's
//! YAMLTree turns the values into events with its style hints, and libyaml's emitter (crate::yaml)
//! picks the final style and writes them.

use crate::config::value::{Map, Value, tokenizes_to_string};
use crate::yaml::ScalarStyle;
use crate::yaml::emitter::{Event, emit};
use regex::Regex;
use std::sync::LazyLock;

/// The mapping as Psych dumps it, without the leading `---` line.
// ponytail: a value shared through a YAML alias is written out again where Psych writes `&1`/`*1`.
pub fn dump(map: &Map) -> String {
    let mut events = vec![Event::StreamStart, Event::DocumentStart { implicit: false }];
    visit_map(&mut events, map);
    events.extend([Event::DocumentEnd { implicit: true }, Event::StreamEnd]);
    let text = emit(&events);
    text.split_once('\n').map_or(String::new(), |(_, rest)| rest.to_string())
}

fn visit_map(events: &mut Vec<Event>, map: &Map) {
    events.push(Event::MappingStart { tag: None, implicit: true, flow: false });
    for (k, v) in &map.0 {
        // Keys do not keep the Symbol/String distinction; a `:name` key is taken as a Symbol.
        let key =
            if k.chars().count() > 1 && k.starts_with(':') { Value::Sym(k.clone()) } else { Value::Str(k.clone()) };
        visit(events, &key);
        visit(events, v);
    }
    events.push(Event::MappingEnd);
}

fn scalar(tag: Option<&str>, value: String, plain: bool, quoted: bool, style: ScalarStyle) -> Event {
    Event::Scalar { tag: tag.map(String::from), value, plain_implicit: plain, quoted_implicit: quoted, style }
}

fn visit(events: &mut Vec<Event>, v: &Value) {
    let event = match v {
        Value::Map(m) => return visit_map(events, m),
        Value::Seq(items) => {
            events.push(Event::SequenceStart { tag: None, implicit: true, flow: false });
            for item in items.iter() {
                visit(events, item);
            }
            events.push(Event::SequenceEnd);
            return;
        }
        Value::Null => scalar(Some("tag:yaml.org,2002:null"), String::new(), true, false, ScalarStyle::Any),
        Value::Bool(b) => scalar(None, b.to_string(), true, false, ScalarStyle::Any),
        Value::Int(i) => scalar(None, i.to_string(), true, false, ScalarStyle::Any),
        Value::Float(f) => {
            let text = if f.is_nan() {
                ".nan".into()
            } else if f.is_infinite() {
                (if *f > 0.0 { ".inf" } else { "-.inf" }).into()
            } else {
                crate::helpers::ruby_string::float_to_s(*f)
            };
            scalar(None, text, true, false, ScalarStyle::Any)
        }
        Value::Regex(r) => scalar(Some("!ruby/regexp"), r.clone(), false, false, ScalarStyle::Any),
        // An empty Symbol (`!ruby/sym ''`) is the only `Sym(":")`.
        Value::Sym(s) if s == ":" => scalar(Some("!ruby/symbol"), String::new(), false, false, ScalarStyle::Any),
        Value::Sym(s) => scalar(None, s.clone(), true, false, ScalarStyle::Any),
        Value::Str(s) => visit_string(s),
    };
    events.push(event);
}

/// Psych::Visitors::YAMLTree#visit_String: a literal block for a line break not ending the
/// string, `!!str '<<'`, a quoted `y`/`n`, double quotes for a line starting with a non-word
/// character, single quotes for what would not load back as this String.
fn visit_string(s: &str) -> Event {
    static NON_WORD_LINE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?m)^[^\w][^"]*$"#).unwrap());
    static OCTALISH: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\A0[0-7]*[89]").unwrap());
    let inner_break = s.char_indices().any(|(i, c)| c == '\n' && !matches!(&s[i + 1..], "" | "\n"));
    let (style, tag, plain) = if inner_break {
        (ScalarStyle::Literal, None, true)
    } else if s == "<<" {
        (ScalarStyle::SingleQuoted, Some("tag:yaml.org,2002:str"), false)
    } else if matches!(s, "y" | "Y" | "n" | "N") || NON_WORD_LINE.is_match(s) {
        (ScalarStyle::DoubleQuoted, None, true)
    } else if !tokenizes_to_string(s) || OCTALISH.is_match(s) {
        (ScalarStyle::SingleQuoted, None, true)
    } else {
        (ScalarStyle::Plain, None, true)
    };
    scalar(tag, s.to_string(), plain, plain, style)
}
#[cfg(test)]
mod tests {
    use super::*;

    // RuboCop 1.91.0 `--show-cops` (Psych dump) of `Max: 0.00001`, `1.0e+20`,
    // `12345678901234567.0`, `5.0`, `0.5`.
    #[test]
    fn floats_dump_as_float_to_s() {
        let f = |x: f64| dump(&Map(vec![("k".into(), Value::Float(x))]));
        assert_eq!(f(0.00001), "k: 1.0e-05\n");
        assert_eq!(f(1.0e20), "k: 1.0e+20\n");
        assert_eq!(f(12345678901234567.0), "k: 1.2345678901234568e+16\n");
        assert_eq!(f(5.0), "k: 5.0\n");
        assert_eq!(f(0.5), "k: 0.5\n");
    }

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
            ("l", Value::Seq(vec![].into())),
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

    // Ruby 4.0.6 `{"a" => {...}, "z" => 1}.to_yaml` (#510).
    #[test]
    fn multi_line_strings_as_psych_dumps_them() {
        let m = |pairs: Vec<(&str, Value)>| {
            Value::Map(Map(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect()).into())
        };
        let s = |v: &str| Value::Str(v.into());
        let inner = m(vec![
            ("D", s("x\ny")),
            ("E", s("x \ny")),
            ("F", s("x\ny ")),
            ("G", s(" x\ny")),
            ("H", s("x\ny\n")),
            ("I", s("x\ny\n\n")),
            ("J", s("x\n\n\n")),
            ("K", s("\nx\ny")),
            ("L", s("x\n\ny")),
            ("M", Value::Seq(vec![s("p\nq"), Value::Int(1)].into())),
            ("O", s("x\ty\nz")),
        ]);
        let Value::Map(map) = m(vec![("a", inner), ("z", Value::Int(1))]) else { unreachable!() };
        let expected = "a:\n  D: |-\n    x\n    y\n  E: \"x \\ny\"\n  F: \"x\\ny \"\n  G: |2-\n     x\n    y\n  H: |\n    x\n    \
            y\n  I: |+\n    x\n    y\n\n  J: |+\n    x\n\n\n  K: |2-\n\n    x\n    y\n  L: |-\n    x\n\n    y\n  M:\n  - |-\n    \
            p\n    q\n  - 1\n  O: \"x\\ty\\nz\"\nz: 1\n";
        assert_eq!(dump(&map), expected);
    }

    #[test]
    fn quoted_colon_string_is_not_a_symbol() {
        let Value::Map(m) = crate::config::value::parse_yaml("a: ':x'\nb: :x\nc:\n- \":y\"\n- :y\n").unwrap() else {
            panic!()
        };
        assert_eq!(dump(&m), "a: \":x\"\nb: :x\nc:\n- \":y\"\n- :y\n");
        assert_eq!(m.str("b"), Some(":x"));
    }
}
