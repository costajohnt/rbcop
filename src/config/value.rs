//! Config values as RuboCop sees them after Psych: insertion-ordered maps, plus Regexp.

use regex::Regex;
use yaml_rust2::Yaml;
use yaml_rust2::parser::{Event, MarkedEventReceiver, Parser};
use yaml_rust2::scanner::{Marker, TScalarStyle};

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    /// `!ruby/regexp /source/flags`, kept as written.
    Regex(String),
    Seq(Vec<Value>),
    Map(Map),
}

/// Insertion-ordered map with unique keys (Ruby Hash).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Map(pub Vec<(String, Value)>);

impl Map {
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.0.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    pub fn get_mut(&mut self, key: &str) -> Option<&mut Value> {
        self.0.iter_mut().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    pub fn contains(&self, key: &str) -> bool {
        self.get(key).is_some()
    }

    /// Set a key, keeping its position if it already exists (Hash#[]=).
    pub fn insert(&mut self, key: impl Into<String>, value: Value) {
        let key = key.into();
        match self.get_mut(&key) {
            Some(v) => *v = value,
            None => self.0.push((key, value)),
        }
    }

    pub fn remove(&mut self, key: &str) -> Option<Value> {
        let i = self.0.iter().position(|(k, _)| k == key)?;
        Some(self.0.remove(i).1)
    }

    pub fn map(&self, key: &str) -> Option<&Map> {
        match self.get(key) {
            Some(Value::Map(m)) => Some(m),
            _ => None,
        }
    }

    pub fn map_mut(&mut self, key: &str) -> Option<&mut Map> {
        match self.get_mut(key) {
            Some(Value::Map(m)) => Some(m),
            _ => None,
        }
    }

    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.0.iter().map(|(k, _)| k.as_str())
    }

    /// Ruby truthiness of a key's value (`nil`/missing and `false` are falsy).
    pub fn truthy(&self, key: &str) -> bool {
        !matches!(self.get(key), None | Some(Value::Null) | Some(Value::Bool(false)))
    }

    pub fn str(&self, key: &str) -> Option<&str> {
        match self.get(key) {
            Some(Value::Str(s)) => Some(s),
            _ => None,
        }
    }

    pub fn bool(&self, key: &str) -> Option<bool> {
        match self.get(key) {
            Some(Value::Bool(b)) => Some(*b),
            _ => None,
        }
    }

    pub fn int(&self, key: &str) -> Option<i64> {
        match self.get(key) {
            Some(Value::Int(i)) => Some(*i),
            Some(Value::Float(f)) => Some(*f as i64),
            _ => None,
        }
    }

    /// Strings of an Array value (or a single String), like `Array(cop_config[key])`.
    pub fn strings(&self, key: &str) -> Vec<String> {
        match self.get(key) {
            Some(Value::Seq(items)) => items.iter().filter_map(Value::as_string).collect(),
            Some(v) => v.as_string().into_iter().collect(),
            None => Vec::new(),
        }
    }
}

impl Value {
    pub fn as_string(&self) -> Option<String> {
        match self {
            Value::Str(s) => Some(s.clone()),
            Value::Int(i) => Some(i.to_string()),
            Value::Float(f) => Some(f.to_string()),
            Value::Bool(b) => Some(b.to_string()),
            Value::Regex(r) => Some(r.clone()),
            _ => None,
        }
    }

    pub fn as_map(&self) -> Option<&Map> {
        match self {
            Value::Map(m) => Some(m),
            _ => None,
        }
    }
}

const REGEX_MARK: &str = "\u{1}rbcop-regexp:";

/// Parse a YAML document the way Psych + RuboCop's restricted loader would.
pub fn parse_yaml(text: &str) -> Result<Value, String> {
    let text = mark_regexps(text);
    let mut loader = Loader::default();
    Parser::new(text.chars()).load(&mut loader, true).map_err(|e| e.to_string())?;
    Ok(loader.docs.into_iter().next().map_or(Value::Map(Map::default()), convert))
}

/// yaml-rust2's YamlLoader, except that a duplicated key replaces the earlier value in place, as
/// Psych does (YamlLoader rejects the document).
#[derive(Default)]
struct Loader {
    docs: Vec<Yaml>,
    /// Open collections with their anchor ids.
    stack: Vec<(Yaml, usize)>,
    /// The pending key of each open mapping (BadValue: the next node is a key).
    keys: Vec<Yaml>,
    anchors: std::collections::HashMap<usize, Yaml>,
}

impl MarkedEventReceiver for Loader {
    fn on_event(&mut self, ev: Event, _: Marker) {
        match ev {
            Event::DocumentEnd => {
                let doc = self.stack.pop().map_or(Yaml::BadValue, |n| n.0);
                self.docs.push(doc);
            }
            Event::SequenceStart(aid, _) => self.stack.push((Yaml::Array(Vec::new()), aid)),
            Event::MappingStart(aid, _) => {
                self.stack.push((Yaml::Hash(Default::default()), aid));
                self.keys.push(Yaml::BadValue);
            }
            Event::SequenceEnd | Event::MappingEnd => {
                if matches!(ev, Event::MappingEnd) {
                    self.keys.pop();
                }
                if let Some(node) = self.stack.pop() {
                    self.insert(node);
                }
            }
            Event::Scalar(v, style, aid, tag) => {
                // Quoted scalars are strings; `!!str` and other tags as YamlLoader treats them.
                let node = match (style, tag) {
                    (TScalarStyle::Plain, Some(t)) if t.handle == "tag:yaml.org,2002:" => match t.suffix.as_str() {
                        "str" => Yaml::String(v),
                        _ => Yaml::from_str(&v),
                    },
                    (TScalarStyle::Plain, Some(_)) => Yaml::String(v),
                    (TScalarStyle::Plain, None) => Yaml::from_str(&v),
                    _ => Yaml::String(v),
                };
                self.insert((node, aid));
            }
            Event::Alias(id) => {
                let node = self.anchors.get(&id).cloned().unwrap_or(Yaml::BadValue);
                self.insert((node, 0));
            }
            _ => {}
        }
    }
}

impl Loader {
    fn insert(&mut self, (node, aid): (Yaml, usize)) {
        if aid > 0 {
            self.anchors.insert(aid, node.clone());
        }
        match self.stack.last_mut() {
            None => self.stack.push((node, aid)),
            Some((Yaml::Array(items), _)) => items.push(node),
            Some((Yaml::Hash(h), _)) => {
                let key = self.keys.last_mut().expect("a mapping has a key slot");
                if key.is_badvalue() {
                    *key = node;
                } else {
                    let key = std::mem::replace(key, Yaml::BadValue);
                    match h.get_mut(&key) {
                        Some(v) => *v = node,
                        None => {
                            h.insert(key, node);
                        }
                    }
                }
            }
            Some(_) => unreachable!("only collections are open"),
        }
    }
}

/// yaml-rust2 drops tags, so turn `!ruby/regexp /x/i` into a marked string first.
fn mark_regexps(text: &str) -> String {
    if !text.contains("!ruby/regexp") {
        return text.to_string();
    }
    // The literal may be plain, 'single-quoted' ('' escapes ') or "double-quoted" (\ escapes).
    let re = Regex::new(r#"!ruby/regexp\s+(?:'((?:[^']|'')*)'|"((?:[^"\\]|\\.)*)"|(/.*/[a-z]*))"#).unwrap();
    re.replace_all(text, |c: &regex::Captures| {
        let literal = match (c.get(1), c.get(2), c.get(3)) {
            (Some(s), _, _) => s.as_str().replace("''", "'"),
            // ponytail: only \\ and \" are unescaped; other YAML escapes are rare in a regexp.
            (_, Some(d), _) => d.as_str().replace(r#"\""#, "\"").replace(r"\\", r"\"),
            (_, _, Some(p)) => p.as_str().to_string(),
            _ => unreachable!(),
        };
        format!("'{REGEX_MARK}{}'", literal.replace('\'', "''"))
    })
    .into_owned()
}

fn convert(y: Yaml) -> Value {
    match y {
        Yaml::Null | Yaml::BadValue | Yaml::Alias(_) => Value::Null,
        Yaml::Boolean(b) => Value::Bool(b),
        Yaml::Integer(i) => Value::Int(i),
        Yaml::Real(r) => match r.as_str() {
            ".inf" | ".Inf" | ".INF" | "+.inf" | "+.Inf" | "+.INF" => Value::Float(f64::INFINITY),
            "-.inf" | "-.Inf" | "-.INF" => Value::Float(f64::NEG_INFINITY),
            ".nan" | ".NaN" | ".NAN" => Value::Float(f64::NAN),
            _ => r.parse().map_or(Value::Str(r), Value::Float),
        },
        Yaml::String(s) => match s.strip_prefix(REGEX_MARK) {
            Some(re) => Value::Regex(re.to_string()),
            None => Value::Str(s),
        },
        Yaml::Array(items) => Value::Seq(items.into_iter().map(convert).collect()),
        Yaml::Hash(h) => {
            let mut map = Map::default();
            for (k, v) in h {
                // Psych's merge key: `<<: *anchor` (or a list of them, earlier ones winning) merges
                // the mapping in, overriding keys before it; keys after it override the merged ones.
                if k.as_str() == Some("<<") && matches!(v, Yaml::Hash(_) | Yaml::Array(_)) {
                    let sources = if let Yaml::Array(list) = v { list } else { vec![v] };
                    let mut merged = Map::default();
                    for source in sources.into_iter().rev() {
                        if let Value::Map(m) = convert(source) {
                            for (k, v) in m.0 {
                                merged.insert(k, v);
                            }
                        }
                    }
                    for (k, v) in merged.0 {
                        map.insert(k, v);
                    }
                    continue;
                }
                let key = match k {
                    Yaml::String(s) => s,
                    Yaml::Integer(i) => i.to_string(),
                    Yaml::Boolean(b) => b.to_string(),
                    Yaml::Real(r) => r,
                    _ => continue,
                };
                // Psych keeps the last of duplicate keys.
                map.insert(key, convert(v));
            }
            Value::Map(map)
        }
    }
}

/// Compile a Ruby regexp literal (`/src/flags`) or bare source into a Rust regex.
// ponytail: Ruby-only syntax (lookbehind, \h, possessive) fails to compile and the pattern is skipped.
pub fn ruby_regex(literal: &str) -> Option<Regex> {
    let (source, flags) = match literal.strip_prefix('/').and_then(|r| r.rsplit_once('/')) {
        Some((src, flags)) => (src, flags),
        None => (literal, ""),
    };
    let mut prefix = String::new();
    for f in flags.chars() {
        match f {
            'i' => prefix.push('i'),
            'm' => prefix.push('s'),
            'x' => prefix.push('x'),
            _ => {}
        }
    }
    // Ruby's ^ and $ are always line anchors.
    prefix.push('m');
    // Ruby's \w \d \s \h are ASCII-only (Rust's are Unicode); \Z allows a final newline.
    Regex::new(&format!("(?{prefix}){}", crate::helpers::ruby_string::ruby_regex_classes(source))).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Expected values from Ruby 4.0 `YAML.safe_load(..., aliases: true)` (Psych).
    #[test]
    fn duplicate_and_merge_keys_like_psych() {
        let y =
            "A: 1\nB: 2\nA: 3\nbase: &b\n  Max: 5\n  X: 1\nC:\n  <<: *b\n  Max: 7\nD:\n  <<: [*b, {Max: 9, Y: 2}]\n";
        let Value::Map(m) = parse_yaml(y).unwrap() else { panic!("not a map") };
        let keys: Vec<&str> = m.keys().collect();
        assert_eq!(keys, ["A", "B", "base", "C", "D"]);
        assert_eq!(m.get("A"), Some(&Value::Int(3)));
        let c = m.map("C").unwrap();
        assert_eq!((c.int("Max"), c.int("X")), (Some(7), Some(1)));
        let d = m.map("D").unwrap();
        assert_eq!((d.int("Max"), d.int("X"), d.int("Y")), (Some(5), Some(1), Some(2)));
        let Value::Map(t) = parse_yaml("x: !!str 1\n'y': \"2\"\n").unwrap() else { panic!("not a map") };
        assert_eq!((t.get("x"), t.get("y")), (Some(&Value::Str("1".into())), Some(&Value::Str("2".into()))));
    }
}
