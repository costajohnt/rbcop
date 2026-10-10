//! Config values as RuboCop sees them after Psych: insertion-ordered maps, plus Regexp.

use regex::Regex;
use std::sync::Arc;
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
    /// A plain `:name` scalar, which Psych loads as a Symbol, kept as written (colon included). Readers
    /// see it as the string it always was; only `--show-cops` dumps it differently from a quoted one.
    Sym(String),
    /// `!ruby/regexp /source/flags`, kept as written.
    Regex(String),
    /// Shared, as Psych shares an aliased node (copy on write: `Arc::make_mut`).
    Seq(Arc<Vec<Value>>),
    Map(Arc<Map>),
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
            Some(Value::Map(m)) => Some(Arc::make_mut(m)),
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
            Some(Value::Str(s) | Value::Sym(s)) => Some(s),
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
            Value::Str(s) | Value::Sym(s) => Some(s.clone()),
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

const SYM_MARK: &str = "\u{1}rbcop-symbol:";

/// Parse a YAML document the way Psych + RuboCop's restricted loader would.
pub fn parse_yaml(text: &str) -> Result<Value, String> {
    let text = mark_regexps(text);
    let mut loader = Loader::default();
    Parser::new(text.chars()).load(&mut loader, true).map_err(|e| e.to_string())?;
    Ok(loader.docs.into_iter().next().unwrap_or_else(|| Value::Map(Default::default())))
}

/// A mapping key: the scalar as yaml-rust2 resolved it (keys compare as YAML nodes), or `None` for a
/// collection key, which the converted Hash leaves out.
type Key = Option<Yaml>;

/// An open collection.
enum Open {
    Seq(Vec<Value>),
    /// Entries so far (a duplicated key replaces the earlier value in place, as Psych does), and the
    /// key waiting for its value (`None`: the next node is a key).
    Map(Vec<(Key, Value)>, Option<Key>),
}

/// yaml-rust2's YamlLoader, except that it builds `Value`s directly, a duplicated key replaces the
/// earlier value in place, as Psych does (YamlLoader rejects the document), a plain `:name` scalar
/// (a Symbol to Psych's ScalarScanner) is marked, so it can be told apart from a quoted ':name'
/// String, and an alias shares its anchor's collection (`Arc`) instead of copying it, as Psych's
/// aliases are the same object: copying makes a few lines of nested aliases exponential.
#[derive(Default)]
struct Loader {
    docs: Vec<Value>,
    /// Open collections with their anchor ids.
    stack: Vec<(Open, usize)>,
    anchors: std::collections::HashMap<usize, (Key, Value)>,
    docs_ended: usize,
}

impl MarkedEventReceiver for Loader {
    fn on_event(&mut self, ev: Event, _: Marker) {
        match ev {
            Event::DocumentEnd => {
                // `insert` pushes a finished root node; an empty document is nil.
                self.docs_ended += 1;
                if self.docs.len() < self.docs_ended {
                    self.docs.push(Value::Null);
                }
            }
            Event::SequenceStart(aid, _) => self.stack.push((Open::Seq(Vec::new()), aid)),
            Event::MappingStart(aid, _) => self.stack.push((Open::Map(Vec::new(), None), aid)),
            Event::SequenceEnd | Event::MappingEnd => {
                if let Some((open, aid)) = self.stack.pop() {
                    let value = match open {
                        Open::Seq(items) => Value::Seq(Arc::new(items)),
                        Open::Map(entries, _) => Value::Map(Arc::new(to_map(entries))),
                    };
                    self.insert(None, value, aid);
                }
            }
            Event::Scalar(v, style, aid, tag) => {
                // Quoted scalars are strings; `!!str` and other tags as YamlLoader treats them.
                let node = match (style, tag) {
                    (TScalarStyle::Plain, None) if v.chars().count() > 1 && v.starts_with(':') => {
                        Yaml::String(format!("{SYM_MARK}{v}"))
                    }
                    (TScalarStyle::Plain, Some(t)) if t.handle == "tag:yaml.org,2002:" => match t.suffix.as_str() {
                        "str" => Yaml::String(v),
                        _ => Yaml::from_str(&v),
                    },
                    (TScalarStyle::Plain, Some(_)) => Yaml::String(v),
                    (TScalarStyle::Plain, None) => Yaml::from_str(&v),
                    _ => Yaml::String(v),
                };
                let value = scalar(node.clone());
                self.insert(Some(node), value, aid);
            }
            Event::Alias(id) => {
                let (key, value) = self.anchors.get(&id).cloned().unwrap_or((Some(Yaml::BadValue), Value::Null));
                self.insert(key, value, 0);
            }
            _ => {}
        }
    }
}

impl Loader {
    fn insert(&mut self, key: Key, value: Value, aid: usize) {
        if aid > 0 {
            self.anchors.insert(aid, (key.clone(), value.clone()));
        }
        match self.stack.last_mut() {
            None => self.docs.push(value),
            Some((Open::Seq(items), _)) => items.push(value),
            Some((Open::Map(entries, pending), _)) => match pending.take() {
                None => *pending = Some(key),
                Some(k) => match entries.iter_mut().find(|(e, _)| k.is_some() && *e == k) {
                    Some((_, v)) => *v = value,
                    None => entries.push((k, value)),
                },
            },
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

/// A scalar node as Psych loads it.
fn scalar(y: Yaml) -> Value {
    match y {
        Yaml::Boolean(b) => Value::Bool(b),
        Yaml::Integer(i) => Value::Int(i),
        Yaml::Real(r) => match r.as_str() {
            ".inf" | ".Inf" | ".INF" | "+.inf" | "+.Inf" | "+.INF" => Value::Float(f64::INFINITY),
            "-.inf" | "-.Inf" | "-.INF" => Value::Float(f64::NEG_INFINITY),
            ".nan" | ".NaN" | ".NAN" => Value::Float(f64::NAN),
            _ => r.parse().map_or(Value::Str(r), Value::Float),
        },
        Yaml::String(s) => match (s.strip_prefix(REGEX_MARK), s.strip_prefix(SYM_MARK)) {
            (Some(re), _) => Value::Regex(re.to_string()),
            (_, Some(sym)) => Value::Sym(sym.to_string()),
            _ => Value::Str(s),
        },
        _ => Value::Null,
    }
}

/// A finished mapping as a Hash: string keys, merge keys applied.
fn to_map(entries: Vec<(Key, Value)>) -> Map {
    let mut map = Map::default();
    for (k, v) in entries {
        let Some(k) = k else { continue };
        // Psych's merge key: `<<: *anchor` (or a list of them, earlier ones winning) merges
        // the mapping in, overriding keys before it; keys after it override the merged ones.
        if k.as_str() == Some("<<") && matches!(v, Value::Map(_) | Value::Seq(_)) {
            let sources = match &v {
                Value::Seq(list) => list.iter().collect(),
                _ => vec![&v],
            };
            let mut merged = Map::default();
            for source in sources.into_iter().rev() {
                if let Value::Map(m) = source {
                    for (k, v) in &m.0 {
                        merged.insert(k.clone(), v.clone());
                    }
                }
            }
            for (k, v) in merged.0 {
                map.insert(k, v);
            }
            continue;
        }
        let key = match k {
            // ponytail: a Symbol key and a quoted ':name' key both become the String key.
            Yaml::String(s) => s.strip_prefix(SYM_MARK).map_or(s.clone(), String::from),
            Yaml::Integer(i) => i.to_string(),
            Yaml::Boolean(b) => b.to_string(),
            Yaml::Real(r) => r,
            _ => continue,
        };
        // Psych keeps the last of duplicate keys.
        map.insert(key, v);
    }
    map
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

    // Psych shares an aliased node, so nested aliases cost what the text costs: 30 lines that
    // expand to 10^31 leaves load at once (copying made 8 lines take minutes). A merge key and
    // a later override on the sharing mapping leave the anchored mapping as it was.
    #[test]
    fn alias_bomb_is_shared_not_copied() {
        let mut y = String::from("a0: &a0 [1,2,3,4,5,6,7,8,9,10]\n");
        for i in 1..=30 {
            let p = format!("*a{}", i - 1);
            y.push_str(&format!("a{i}: &a{i} [{}]\n", [p.as_str(); 10].join(",")));
        }
        y.push_str("base: &b {k: *a30, Max: 3}\nderived:\n  <<: *b\n  Max: 1\n");
        let start = std::time::Instant::now();
        let Value::Map(m) = parse_yaml(&y).unwrap() else { panic!("not a map") };
        assert!(start.elapsed() < std::time::Duration::from_secs(2), "{:?}", start.elapsed());
        let (base, derived) = (m.map("base").unwrap(), m.map("derived").unwrap());
        assert_eq!((base.int("Max"), derived.int("Max")), (Some(3), Some(1)));
        let (Some(Value::Seq(a)), Some(Value::Seq(b))) = (base.get("k"), derived.get("k")) else { panic!("k") };
        assert!(Arc::ptr_eq(a, b));
        assert_eq!(a.len(), 10);
    }
}
