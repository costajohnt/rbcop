//! Config values as RuboCop sees them after Psych: insertion-ordered maps, plus Regexp.

use regex::Regex;
use yaml_rust2::parser::{MarkedEventReceiver, Parser};
use yaml_rust2::scanner::{Marker, TScalarStyle};
use yaml_rust2::{Event, Yaml, YamlLoader};

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

/// YamlLoader, except that a plain `:name` scalar (a Symbol to Psych's ScalarScanner) is marked, so
/// it can be told apart from a quoted ':name' String.
struct Loader(YamlLoader);

impl MarkedEventReceiver for Loader {
    fn on_event(&mut self, ev: Event, mark: Marker) {
        let ev = match ev {
            Event::Scalar(v, TScalarStyle::Plain, aid, None) if v.chars().count() > 1 && v.starts_with(':') => {
                Event::Scalar(format!("{SYM_MARK}{v}"), TScalarStyle::SingleQuoted, aid, None)
            }
            ev => ev,
        };
        self.0.on_event(ev, mark);
    }
}

/// Parse a YAML document the way Psych + RuboCop's restricted loader would.
pub fn parse_yaml(text: &str) -> Result<Value, String> {
    let text = mark_regexps(text);
    let mut loader = Loader(YamlLoader::default());
    Parser::new_from_str(&text).load(&mut loader, true).map_err(|e| e.to_string())?;
    match loader.0.documents().first() {
        Some(doc) => Ok(convert(doc.clone())),
        // No document: an empty file, or an error YamlLoader keeps to itself (a duplicate key);
        // loading again the plain way reports it.
        None => {
            YamlLoader::load_from_str(&text).map_err(|e| e.to_string())?;
            Ok(Value::Map(Map::default()))
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
        Yaml::String(s) => match (s.strip_prefix(REGEX_MARK), s.strip_prefix(SYM_MARK)) {
            (Some(re), _) => Value::Regex(re.to_string()),
            (_, Some(sym)) => Value::Sym(sym.to_string()),
            _ => Value::Str(s),
        },
        Yaml::Array(items) => Value::Seq(items.into_iter().map(convert).collect()),
        Yaml::Hash(h) => {
            let mut map = Map::default();
            for (k, v) in h {
                let key = match k {
                    // ponytail: a Symbol key and a quoted ':name' key both become the String key.
                    Yaml::String(s) => s.strip_prefix(SYM_MARK).map_or(s.clone(), String::from),
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
