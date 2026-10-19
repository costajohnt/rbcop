//! Config values as RuboCop sees them after Psych: insertion-ordered maps, plus Regexp.

use crate::yaml::parser::{Event, Parser};
use regex::Regex;
use std::borrow::Cow;
use std::sync::{Arc, LazyLock};

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
    Regex(Arc<String>),
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

    /// `insert` for each pair, with the keys indexed once (linear, where one `insert` scans).
    pub fn insert_all(&mut self, pairs: impl IntoIterator<Item = (String, Value)>) {
        let mut index: std::collections::HashMap<String, usize> =
            self.0.iter().enumerate().map(|(i, (k, _))| (k.clone(), i)).collect();
        for (k, v) in pairs {
            match index.get(&k) {
                Some(&i) => self.0[i].1 = v,
                None => {
                    index.insert(k.clone(), self.0.len());
                    self.0.push((k, v));
                }
            }
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

    /// A numeric setting a cop reads bare (`cop_config['Max']`) and compares, adds or formats: an
    /// Integer or a Float as written; a String or Symbol one (`Max: "80"`) raises there, a cop
    /// error at the node it was visiting.
    // ponytail: true / false / a list read as missing (the cop's default), as before.
    pub fn num(&self, key: &str) -> Option<Num> {
        match self.get(key) {
            Some(Value::Int(i)) => Some(Num::Int(*i)),
            Some(Value::Float(f)) => Some(Num::Float(*f)),
            Some(Value::Str(_) | Value::Sym(_)) => raise_in_cop("comparison of Integer with String failed"),
            _ => None,
        }
    }

    /// `Integer(self.fetch(key, default))`: a Float truncates, a String parses as Ruby's
    /// `Integer()` does; a Symbol, another String, nil or a boolean raises (a cop error).
    pub fn integer_fetch(&self, key: &str, default: i64) -> i64 {
        match self.get(key) {
            None => default,
            Some(v) => kernel_integer(v).unwrap_or(default),
        }
    }

    /// `Integer(self[key] || default)`: as `integer_fetch`, with nil and false reading the default.
    pub fn integer_or(&self, key: &str, default: i64) -> i64 {
        match self.get(key) {
            None | Some(Value::Null | Value::Bool(false)) => default,
            Some(v) => kernel_integer(v).unwrap_or(default),
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

/// A numeric setting as Ruby holds it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Num {
    Int(i64),
    Float(f64),
}

impl Num {
    pub fn f64(self) -> f64 {
        match self {
            Num::Int(i) => i as f64,
            Num::Float(f) => f,
        }
    }

    /// `Integer(num)`, `%d`, `String#*`: a Float truncates toward zero (NaN and the infinities
    /// raise in a cop).
    pub fn to_i(self) -> i64 {
        match self {
            Num::Int(i) => i,
            Num::Float(f) if f.is_finite() => f as i64,
            Num::Float(_) => raise_in_cop("FloatDomainError").unwrap_or(i64::MAX),
        }
    }
}

/// `to_s` / `#{}`: `3`, `1.5`, `2.0`.
impl std::fmt::Display for Num {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Num::Int(i) => write!(f, "{i}"),
            Num::Float(x) => f.write_str(&crate::helpers::ruby_string::float_to_s(*x)),
        }
    }
}

/// A raise in a cop is a cop error at the node it was visiting (runner::guarded); elsewhere the
/// read is taken as missing.
fn raise_in_cop<T>(msg: &str) -> Option<T> {
    if crate::runner::in_cop() {
        panic!("{msg}");
    }
    None
}

/// `Integer(value)` (raising in a cop when Ruby's does).
fn kernel_integer(v: &Value) -> Option<i64> {
    match v {
        Value::Int(i) => Some(*i),
        Value::Float(f) => Some(Num::Float(*f).to_i()),
        Value::Str(s) => ruby_integer(s).or_else(|| raise_in_cop("invalid value for Integer()")),
        _ => raise_in_cop("can't convert into Integer"),
    }
}

/// `Integer(string)`: surrounding whitespace, a sign, a `0x` / `0b` / `0o` / `0d` / `0` radix
/// prefix and single underscores between digits.
// ponytail: a Bignum saturates at i64::MAX.
pub fn ruby_integer(s: &str) -> Option<i64> {
    let ws = |c: char| matches!(c, ' ' | '\t' | '\n' | '\x0b' | '\x0c' | '\r');
    let t = s.trim_matches(ws);
    let (neg, t) = match t.as_bytes().first() {
        Some(b'-') => (true, &t[1..]),
        Some(b'+') => (false, &t[1..]),
        _ => (false, t),
    };
    let prefix = t.get(..2).map(|p| p.to_ascii_lowercase());
    let (radix, digits) = match prefix.as_deref() {
        Some("0x") => (16, &t[2..]),
        Some("0b") => (2, &t[2..]),
        Some("0o") => (8, &t[2..]),
        Some("0d") => (10, &t[2..]),
        _ if t.len() > 1 && t.starts_with('0') => (8, t),
        _ => (10, t),
    };
    if digits.is_empty() || digits.starts_with('_') || digits.ends_with('_') || digits.contains("__") {
        return None;
    }
    let clean: String = digits.chars().filter(|&c| c != '_').collect();
    if !clean.chars().all(|c| c.is_digit(radix)) {
        return None;
    }
    let n = clean
        .chars()
        .fold(0i128, |n, c| (n * radix as i128 + c.to_digit(radix).unwrap() as i128).min(i64::MAX as i128));
    Some(if neg { -(n as i64) } else { n as i64 })
}

impl Value {
    pub fn as_string(&self) -> Option<String> {
        match self {
            Value::Str(s) | Value::Sym(s) => Some(s.clone()),
            Value::Int(i) => Some(i.to_string()),
            Value::Float(f) => Some(f.to_string()),
            Value::Bool(b) => Some(b.to_string()),
            Value::Regex(r) => Some(r.to_string()),
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

/// Parse a YAML document the way Psych + RuboCop's restricted loader would.
pub fn parse_yaml(text: &str) -> Result<Value, String> {
    parse_yaml_file(text, "<unknown>")
}

/// `parse_yaml`, with errors worded as RuboCop prints them for the config file `path`: Psych's
/// `(path): <problem> <context> at line L column C`, `An alias referenced an unknown anchor: x`,
/// `Tried to load unspecified class: Date`.
///
/// RuboCop's ConfigLoader parses the whole stream into Psych nodes (YAMLDuplicationChecker, which
/// fails on a mapping key that is no scalar as the mapping ends), then loads the first document
/// with ToRuby, a restricted class loader (Regexp and Symbol) and a ScalarScanner.
pub fn parse_yaml_file(text: &str, path: &str) -> Result<Value, String> {
    parse_yaml_bytes(text.as_bytes(), path)
}

/// `parse_yaml_file` on the file's bytes: what is no UTF-8 fails in the reader as in libyaml.
pub fn parse_yaml_bytes(text: &[u8], path: &str) -> Result<Value, String> {
    match parse_stream(text, path, true)? {
        Some(root) => ToRuby::default().accept(&root),
        None => Ok(Value::Map(Default::default())),
    }
}

/// `YAML.unsafe_load`: the unrestricted class loader, so a date, a timestamp or a `!ruby/...`
/// object loads (as an opaque nil here) where `parse_yaml` refuses it; no RuboCop key check.
// ponytail: a class Ruby cannot resolve (ArgumentError there) loads as nil too.
pub fn unsafe_load(text: &str) -> Result<Value, String> {
    match parse_stream(text.as_bytes(), "<unknown>", false)? {
        Some(root) => ToRuby { unsafe_load: true, ..Default::default() }.accept(&root),
        None => Ok(Value::Null),
    }
}

/// A Psych::Nodes node.
enum Node {
    Scalar { value: String, anchor: Option<String>, tag: Option<String>, quoted: bool },
    Seq { anchor: Option<String>, tag: Option<String>, children: Vec<Node> },
    Map { anchor: Option<String>, tag: Option<String>, children: Vec<Node> },
    Alias(String),
}

impl Node {
    fn class(&self) -> &'static str {
        match self {
            Node::Scalar { .. } => "Scalar",
            Node::Seq { .. } => "Sequence",
            Node::Map { .. } => "Mapping",
            Node::Alias(_) => "Alias",
        }
    }
}

/// Psych's TreeBuilder over libyaml's events (the RuboCop handler's end_mapping check included):
/// the first document's root node. `check_keys`: RuboCop's handler, failing on a non-scalar key.
fn parse_stream(text: &[u8], path: &str, check_keys: bool) -> Result<Option<Node>, String> {
    let mut parser = Parser::new(text);
    let mut open: Vec<Node> = Vec::new();
    let mut first: Option<Node> = None;
    let mut documents = 0;
    let mut too_deep = false;
    loop {
        let node = match parser.next_event().map_err(|e| e.message(path))? {
            Event::StreamEnd => break,
            Event::StreamStart | Event::DocumentStart => continue,
            Event::DocumentEnd => {
                documents += 1;
                continue;
            }
            Event::SequenceStart { anchor, tag } => {
                open.push(Node::Seq { anchor, tag, children: Vec::new() });
                continue;
            }
            Event::MappingStart { anchor, tag } => {
                open.push(Node::Map { anchor, tag, children: Vec::new() });
                continue;
            }
            Event::SequenceEnd | Event::MappingEnd => {
                let node = open.pop().expect("an open collection");
                if let Node::Map { children, .. } = &node
                    && check_keys
                    && let Some(key) = children.iter().step_by(2).find(|k| !matches!(k, Node::Scalar { .. }))
                {
                    return Err(format!("undefined method 'value' for an instance of Psych::Nodes::{}", key.class()));
                }
                node
            }
            Event::Scalar { anchor, tag, value, quoted_implicit, .. } => {
                Node::Scalar { value, anchor, tag, quoted: quoted_implicit }
            }
            Event::Alias(a) => Node::Alias(a),
        };
        let depth = open.len();
        match open.last_mut() {
            Some(_) if depth > MAX_DEPTH => too_deep |= documents == 0,
            Some(Node::Seq { children, .. } | Node::Map { children, .. }) => children.push(node),
            Some(_) => unreachable!(),
            None if documents == 0 && first.is_none() => first = Some(node),
            // ponytail: later documents are parsed (for their errors) and dropped.
            None => {}
        }
    }
    if too_deep {
        return Err("stack level too deep".into());
    }
    Ok(first)
}

/// About where Psych's recursive ToRuby runs out of stack (SystemStackError) in a RuboCop run:
/// nodes nested deeper are not kept, and loading the first document fails.
// ponytail: Ruby's real limit depends on the call stack (about 1000 mappings, 1400 sequences).
const MAX_DEPTH: usize = 1000;

/// Psych::Visitors::ToRuby with RuboCop's ClassLoader::Restricted (Regexp, Symbol) and no
/// load_tags. Errors are the Ruby exception messages RuboCop prints.
#[derive(Default)]
struct ToRuby {
    anchors: std::collections::HashMap<String, Value>,
    /// Psych.unsafe_load's ClassLoader: a refused class loads.
    unsafe_load: bool,
}

fn refused(class: &str) -> Result<Value, String> {
    Err(format!("Tried to load unspecified class: {class}"))
}

/// A Hash key: rbcop's maps have String keys (a Symbol key keeps its colon); nil keys are left out.
fn hash_key(key: &Value) -> Option<String> {
    match key {
        Value::Str(s) | Value::Sym(s) => Some(s.clone()),
        Value::Regex(s) => Some(s.to_string()),
        Value::Int(i) => Some(i.to_string()),
        Value::Float(f) => Some(crate::helpers::ruby_string::float_to_s(*f)),
        Value::Bool(b) => Some(b.to_string()),
        Value::Null | Value::Seq(_) | Value::Map(_) => None,
    }
}

fn merge_into(hash: &mut Map, other: &Map) {
    for (k, v) in &other.0 {
        hash.insert(k.clone(), v.clone());
    }
}

impl ToRuby {
    fn accept(&mut self, node: &Node) -> Result<Value, String> {
        let (anchor, value) = match node {
            Node::Alias(a) => {
                return self
                    .anchors
                    .get(a)
                    .cloned()
                    .ok_or_else(|| format!("An alias referenced an unknown anchor: {a}"));
            }
            Node::Scalar { value, anchor, tag, quoted } => (anchor, self.deserialize(value, tag.as_deref(), *quoted)),
            Node::Seq { anchor, tag, children } => (anchor, self.visit_sequence(tag.as_deref(), children)),
            Node::Map { anchor, tag, children } => (anchor, self.visit_mapping(tag.as_deref(), children)),
        };
        let value = match value {
            Err(e) if self.unsafe_load && e.starts_with("Tried to load unspecified class: ") => Value::Null,
            v => v?,
        };
        // ponytail: registered once built, so an alias inside its own anchored collection is
        // unknown (Psych registers a collection before its children and makes it recursive).
        if let Some(a) = anchor {
            self.anchors.insert(a.clone(), value.clone());
        }
        Ok(value)
    }

    fn deserialize(&mut self, value: &str, tag: Option<&str>, quoted: bool) -> Result<Value, String> {
        static STR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^!(?:str|ruby/string)(?::(.*))?$").unwrap());
        if quoted {
            return Ok(Value::Str(value.into()));
        }
        let Some(tag) = tag else { return tokenize(value, self.unsafe_load) };
        match tag {
            "!binary" | "tag:yaml.org,2002:binary" => Ok(Value::Str(unpack_base64(value))),
            "tag:yaml.org,2002:str" => Ok(Value::Str(value.into())),
            t if STR.is_match(t) => match STR.captures(t).and_then(|c| c.get(1)).map(|m| m.as_str()) {
                Some(class) if !class.is_empty() => refused(class),
                _ => Ok(Value::Str(value.into())),
            },
            "!ruby/object:BigDecimal" => refused("BigDecimal"),
            "!ruby/object:DateTime" => refused("DateTime"),
            "!ruby/object:Complex" => refused("Complex"),
            "!ruby/object:Rational" => refused("Rational"),
            // `resolve_class`: no class for an empty name.
            "!ruby/class" | "!ruby/module" if value.is_empty() => Ok(Value::Null),
            "!ruby/class" | "!ruby/module" => refused(value),
            "tag:yaml.org,2002:float" | "!float" => ruby_float(tokenize(value, self.unsafe_load)?),
            "!ruby/regexp" => {
                // `/^\/(?<string>.*)\/(?<options>[mixn]*)$/m`; Regexp#inspect keeps the source, `/`
                // escaped, and the options in `mix` order.
                static RE: LazyLock<Regex> =
                    LazyLock::new(|| Regex::new(r"(?ms)^/(?<string>.*)/(?<options>[mixn]*)$").unwrap());
                let Some(c) = RE.captures(value) else { return Err("undefined method '[]' for nil".into()) };
                let source = regexp_inspect_source(&c["string"].replace("\\/", "/"));
                let flags: String = "mixn".chars().filter(|f| c["options"].contains(*f)).collect();
                Ok(Value::Regex(Arc::new(format!("/{source}/{flags}"))))
            }
            "!ruby/range" => refused("Range"),
            t if t.starts_with("!ruby/sym") => Ok(Value::Sym(format!(":{value}"))),
            _ => tokenize(value, self.unsafe_load),
        }
    }

    fn visit_sequence(&mut self, tag: Option<&str>, children: &[Node]) -> Result<Value, String> {
        static CLASS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^!(?:seq|ruby/array):(.*)$").unwrap());
        match tag {
            // Psych::Omap, a Hash: each item's first and last child.
            // ponytail: loaded as a plain Map, so `--show-cops` leaves out Psych's `!!omap` tag.
            Some("!omap" | "tag:yaml.org,2002:omap") => {
                let mut map = Map::default();
                for item in children {
                    let (Node::Seq { children: pair, .. } | Node::Map { children: pair, .. }) = item else {
                        return Err(format!(
                            "undefined method 'children' for an instance of Psych::Nodes::{}",
                            item.class()
                        ));
                    };
                    let (Some(k), Some(v)) = (pair.first(), pair.last()) else {
                        return Err(
                            "undefined method 'visit_NilClass' for an instance of Psych::Visitors::ToRuby".into()
                        );
                    };
                    let key = self.accept(k)?;
                    let value = self.accept(v)?;
                    if let Some(key) = hash_key(&key) {
                        map.insert(key, value);
                    }
                }
                Ok(Value::Map(Arc::new(map)))
            }
            Some(t) if CLASS.is_match(t) => match &CLASS.captures(t).expect("matched")[1] {
                "" => Err("undefined method 'allocate' for nil".into()),
                class => refused(class),
            },
            _ => {
                let items = children.iter().map(|c| self.accept(c)).collect::<Result<Vec<_>, _>>()?;
                Ok(Value::Seq(Arc::new(items)))
            }
        }
    }

    /// ToRuby#visit_Psych_Nodes_Mapping's tag cases, in its order.
    fn visit_mapping(&mut self, tag: Option<&str>, children: &[Node]) -> Result<Value, String> {
        static CASES: LazyLock<Vec<Regex>> = LazyLock::new(|| {
            [
                r"^!ruby/struct:?(.*)$",
                r"^!ruby/data(-with-ivars)?(?::(.*))?$",
                r"^!ruby/object:?(.*)$",
                r"^!(?:str|ruby/string)(?::(.*))?$",
                r"^!ruby/array:(.*)$",
                r"^!ruby/exception:?(.*)$",
                r"^!ruby/hash-with-ivars(?::(.*))?$",
                r"^(?:!map|!ruby/hash):(.*)$",
                r"^!ruby/marshalable:(.*)$",
            ]
            .iter()
            .map(|r| Regex::new(r).unwrap())
            .collect()
        });
        let hash = |s: &mut Self| s.revive_hash(children).map(|m| Value::Map(Arc::new(m)));
        let Some(t) = tag else { return hash(self) };
        let Some((case, c)) = CASES.iter().enumerate().find_map(|(i, re)| re.captures(t).map(|c| (i, c))) else {
            return match t {
                "tag:yaml.org,2002:str" => self.revive_string(None, children),
                "!ruby/range" => refused("Range"),
                "!set" | "tag:yaml.org,2002:set" => refused("Psych::Set"),
                "!omap" | "tag:yaml.org,2002:omap" => refused("Psych::Omap"),
                _ => hash(self),
            };
        };
        let group = |i: usize| c.get(i).map(|m| m.as_str().to_string());
        let name = group(1).unwrap_or_default();
        match case {
            // `resolve_class("")` is nil: Struct (a new Struct class) and Object are refused.
            0 if name.is_empty() => refused("Struct"),
            2 if name.is_empty() => refused("Object"),
            2 if name == "Hash" => hash(self),
            0 | 2 => refused(&name),
            1 => match group(2) {
                Some(class) if !class.is_empty() => refused(&class),
                Some(_) => Err("undefined method 'allocate' for nil".into()),
                // The members are loaded before the anonymous Data class is.
                None => {
                    for c in children {
                        self.accept(c)?;
                    }
                    refused("Data")
                }
            },
            3 => self.revive_string(group(1), children),
            5 => {
                // `Hash[*children]` comes before the class.
                for c in children {
                    self.accept(c)?;
                }
                refused(if name.is_empty() { "Exception" } else { &name })
            }
            6 => {
                if let Some(class) = group(1) {
                    return if class.is_empty() {
                        Err("undefined method 'allocate' for nil".into())
                    } else {
                        refused(&class)
                    };
                }
                // ponytail: only the `elements` entry is loaded; instance variables are dropped.
                let mut map = Map::default();
                for pair in children.chunks(2) {
                    if let (Node::Scalar { value, .. }, Some(Node::Map { children, .. })) = (&pair[0], pair.get(1))
                        && value == "elements"
                    {
                        merge_into(&mut map, &self.revive_hash(children)?);
                    }
                }
                Ok(Value::Map(Arc::new(map)))
            }
            _ if name.is_empty() => Err("undefined method 'allocate' for nil".into()),
            _ => refused(&name),
        }
    }

    /// A `!str` mapping: its `str` entry, if any; other entries become instance variables.
    fn revive_string(&mut self, class: Option<String>, children: &[Node]) -> Result<Value, String> {
        if let Some(class) = class.filter(|c| !c.is_empty()) {
            return refused(&class);
        }
        static IVAR: LazyLock<Regex> =
            LazyLock::new(|| Regex::new(r"^@[\p{Alphabetic}_][\p{Alphabetic}\p{Nd}_]*$").unwrap());
        let (mut string, mut ivars) = (Value::Null, Vec::new());
        for pair in children.chunks(2) {
            let key = self.accept(&pair[0])?;
            let value = self.accept(&pair[1])?;
            match key {
                Value::Str(k) if k == "str" => string = value,
                // `:"@#{k.to_s.sub(/^@/, '')}"`
                k => {
                    let s = match &k {
                        Value::Sym(s) => s[1..].to_string(),
                        k => hash_key(k).unwrap_or_default(),
                    };
                    ivars.push(format!("@{}", s.strip_prefix('@').unwrap_or(&s)));
                }
            }
        }
        // init_with sets them in order: a bad name fails before a frozen receiver does.
        let bad =
            |n: &String| (!IVAR.is_match(n)).then(|| format!("'{n}' is not allowed as an instance variable name"));
        let Some(first) = ivars.first() else { return Ok(string) };
        if let Some(e) = bad(first) {
            return Err(e);
        }
        match &string {
            Value::Null => Err("can't modify frozen NilClass: nil".into()),
            Value::Int(_) | Value::Float(_) | Value::Bool(_) | Value::Sym(_) => {
                Err(format!("can't modify frozen {}", ruby_class(&string)))
            }
            _ => ivars.iter().find_map(bad).map_or(Ok(string), Err),
        }
    }

    /// ToRuby#revive_hash: later keys replace earlier ones in place, `<<` merges a mapping (or a
    /// list of them, earlier ones winning) unless its key is tagged `!!str`; a `<<` whose value
    /// will not merge (TypeError) stays a key.
    fn revive_hash(&mut self, children: &[Node]) -> Result<Map, String> {
        // Pairs in Hash#[]= order, inserted at the end (insert_all: linear in the keys).
        let mut hash = Vec::new();
        for pair in children.chunks(2) {
            let (k, v) = (&pair[0], &pair[1]);
            let key = self.accept(k)?;
            let val = self.accept(v)?;
            let str_tagged = matches!(k, Node::Scalar { tag: Some(t), .. } if t == "tag:yaml.org,2002:str");
            if matches!(&key, Value::Str(s) if s == "<<") && !str_tagged {
                let merged = match (v, &val) {
                    (Node::Alias(_) | Node::Map { .. }, Value::Map(m)) => Some(m.as_ref().clone()),
                    (Node::Seq { .. }, Value::Seq(items)) if items.iter().all(|i| matches!(i, Value::Map(_))) => {
                        let mut h = Map::default();
                        h.insert_all(items.iter().rev().filter_map(Value::as_map).flat_map(|m| m.0.iter().cloned()));
                        Some(h)
                    }
                    _ => None,
                };
                match merged {
                    Some(m) => hash.extend(m.0),
                    None => hash.push(("<<".into(), val)),
                }
                continue;
            }
            if let Some(key) = hash_key(&key) {
                hash.push((key, val));
            }
        }
        let mut map = Map::default();
        map.insert_all(hash);
        Ok(map)
    }
}

/// Regexp#inspect's source: an unescaped `/` gets a backslash.
fn regexp_inspect_source(source: &str) -> String {
    let mut out = String::new();
    let mut chars = source.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                out.push(c);
                out.extend(chars.next());
            }
            '/' => out.push_str("\\/"),
            c => out.push(c),
        }
    }
    out
}

/// `Class: inspect` for a FrozenError message.
fn ruby_class(v: &Value) -> String {
    match v {
        Value::Int(i) => format!("Integer: {i}"),
        Value::Float(f) => format!("Float: {}", crate::helpers::ruby_string::float_to_s(*f)),
        Value::Bool(b) => format!("{}Class: {b}", if *b { "True" } else { "False" }),
        Value::Sym(s) => format!("Symbol: {s}"),
        _ => "Object".into(),
    }
}

/// `String#unpack('m')`: base64, skipping other characters, up to the first `=`.
// ponytail: kept as UTF-8 (lossily); Psych keeps a binary String and dumps a non-ASCII one `!binary`.
fn unpack_base64(s: &str) -> String {
    let mut bytes = Vec::new();
    let (mut acc, mut bits) = (0u32, 0);
    for c in s.chars() {
        let v = match c {
            'A'..='Z' => c as u32 - 'A' as u32,
            'a'..='z' => c as u32 - 'a' as u32 + 26,
            '0'..='9' => c as u32 - '0' as u32 + 52,
            '+' => 62,
            '/' => 63,
            '=' => break,
            _ => continue,
        };
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            bytes.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

/// Kernel#Float on a tokenized scalar.
fn ruby_float(v: Value) -> Result<Value, String> {
    match v {
        Value::Float(_) => Ok(v),
        Value::Int(i) => Ok(Value::Float(i as f64)),
        Value::Str(s) => parse_ruby_float(&s).map(Value::Float).ok_or_else(|| {
            format!("invalid value for Float(): {}", crate::helpers::ruby_string::inspect(s.as_bytes()))
        }),
        Value::Null => Err("can't convert nil into Float".into()),
        Value::Bool(b) => Err(format!("can't convert {b} into Float")),
        Value::Sym(_) => Err("can't convert Symbol into Float".into()),
        _ => Err("can't convert Object into Float".into()),
    }
}

/// Kernel#Float's String rules (Ruby 4): blanks around, `_` between digits, a decimal number (`.5`,
/// `5.` and `1.e5` too) or a `0x` hex one.
fn parse_ruby_float(s: &str) -> Option<f64> {
    static DEC: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(concat!(
            r"^[+-]?(?:[0-9]+(?:_[0-9]+)*(?:\.(?:[0-9]+(?:_[0-9]+)*)?)?|\.[0-9]+(?:_[0-9]+)*)",
            r"(?:[eE][+-]?[0-9]+(?:_[0-9]+)*)?$"
        ))
        .unwrap()
    });
    static HEX: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^([+-]?)0[xX]([0-9a-fA-F]+(?:_[0-9a-fA-F]+)*)$").unwrap());
    let t = s.trim_matches([' ', '\t', '\n', '\x0B', '\x0C', '\r']);
    if let Some(c) = HEX.captures(t) {
        let v = u64::from_str_radix(&c[2].replace('_', ""), 16).ok()? as f64;
        return Some(if &c[1] == "-" { -v } else { v });
    }
    if !DEC.is_match(t) {
        return None;
    }
    t.replace('_', "").replace(".e", ".0e").replace(".E", ".0E").parse().ok()
}

/// Psych's `ScalarScanner#tokenize` (YAML 1.1, `strict_integer: false`) with RuboCop's restricted
/// class loader: `yes`/`on`/`off`, `1_000`, `0100` (octal), `1:30` (base 60), `1.`, `:sym`; a
/// timestamp or date fails as its class (Time, Date) is not permitted. With `unsafe_load` (the
/// unrestricted ClassLoader, as YAMLTree's scanner has too) a valid one loads as an opaque nil and
/// an invalid one (ArgumentError, rescued) stays a String.
fn tokenize(s: &str, unsafe_load: bool) -> Result<Value, String> {
    static WORDISH: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?m)^[^0-9.:-]?[\p{Alphabetic}_ \t\r\n\x0B\x0C!@#$%^&*(){}<>|/\\~;=]+").unwrap());
    static NOT_YTONF: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?mi)^[^ytonf~]").unwrap());
    static NULL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?mi)^null$").unwrap());
    static TRUE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?mi)^(yes|true|on)$").unwrap());
    static FALSE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?mi)^(no|false|off)$").unwrap());
    static TIME: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(concat!(
            r"^-?[0-9]{4}-[0-9]{1,2}-[0-9]{1,2}(?:[Tt]|[ \t\r\n\x0B\x0C]+)[0-9]{1,2}:[0-9][0-9]:[0-9][0-9]",
            r"(?:\.[0-9]*)?(?:[ \t\r\n\x0B\x0C]*(?:Z|[-+][0-9]{1,2}:?(?:[0-9][0-9])?))?$"
        ))
        .unwrap()
    });
    static DATE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^[0-9]{4}-(?:1[012]|0[0-9]|[0-9])-(?:[12][0-9]|3[01]|0[0-9]|[0-9])$").unwrap());
    static SEXA_INT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[-+]?[0-9][0-9_]*(:[0-5]?[0-9]){1,2}$").unwrap());
    static SEXA_FLOAT: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^[-+]?[0-9][0-9_]*(:[0-5]?[0-9]){1,2}\.[0-9_]*$").unwrap());
    static FLOAT: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^[-+]?([0-9][0-9_,]*)?\.[0-9]*([eE][-+][0-9]+)?$").unwrap());
    static INT: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(concat!(
            r"^(?:[-+]?0b[_,]*[0-1][0-1_,]*|[-+]?0[_,]*[0-7][0-7_,]*",
            r"|[-+]?(?:0|[1-9](?:[0-9]|,[0-9]|_[0-9])*)|[-+]?0x[_,]*[0-9a-fA-F][0-9a-fA-F_,]*)$"
        ))
        .unwrap()
    });
    if s.is_empty() {
        return Ok(Value::Null);
    }
    if WORDISH.is_match(s) || s.contains('\n') {
        return Ok(if s.chars().count() > 5 || NOT_YTONF.is_match(s) {
            Value::Str(s.into())
        } else if s == "~" || NULL.is_match(s) {
            Value::Null
        } else if TRUE.is_match(s) {
            Value::Bool(true)
        } else if FALSE.is_match(s) {
            Value::Bool(false)
        } else {
            Value::Str(s.into())
        });
    }
    for (re, class, valid) in [(&TIME, "Time", valid_time as fn(&str) -> bool), (&DATE, "Date", valid_date)] {
        if re.is_match(s) {
            return match unsafe_load {
                false => refused(class),
                true if valid(s) => Ok(Value::Null),
                true => Ok(Value::Str(s.into())),
            };
        }
    }
    let lower = s.to_ascii_lowercase();
    match lower.as_str() {
        ".inf" | "+.inf" => return Ok(Value::Float(f64::INFINITY)),
        "-.inf" => return Ok(Value::Float(f64::NEG_INFINITY)),
        ".nan" => return Ok(Value::Float(f64::NAN)),
        _ => {}
    }
    if s.len() > 1 && s.starts_with(':') {
        return Ok(Value::Sym(format!(":{}", symbol_name(s))));
    }
    // `n.to_i * 60 ** (e - 2).abs` (or `n.to_f`) per part, as Psych writes it (so `1:30` is 5400).
    let scale = |e: usize| 60i128.pow((e as i32 - 2).unsigned_abs());
    if SEXA_INT.is_match(s) {
        let i = s.split(':').enumerate().fold(0i128, |i, (e, n)| i.saturating_add(to_i(n).saturating_mul(scale(e))));
        // Psych makes a Bignum; it saturates here (docs/known-gaps.md).
        return Ok(Value::Int(i.clamp(i64::MIN as i128, i64::MAX as i128) as i64));
    }
    if SEXA_FLOAT.is_match(s) {
        return Ok(Value::Float(s.split(':').enumerate().fold(0.0, |i, (e, n)| i + to_f(n) * scale(e) as f64)));
    }
    if FLOAT.is_match(s) {
        if matches!(s, "." | "+." | "-.") {
            return Ok(Value::Str(s.into()));
        }
        let t = s.replace([',', '_'], "");
        let t = t.replacen(".e", "e", 1).replacen(".E", "E", 1);
        let t = t.strip_suffix('.').unwrap_or(&t);
        return ruby_float(Value::Str(t.to_string()));
    }
    if INT.is_match(s) {
        let t = s.replace([',', '_'], "");
        let (neg, t) = match t.as_bytes()[0] {
            b'-' => (true, &t[1..]),
            b'+' => (false, &t[1..]),
            _ => (false, &t[..]),
        };
        let parsed = if let Some(b) = t.strip_prefix("0b") {
            i64::from_str_radix(b, 2)
        } else if let Some(h) = t.strip_prefix("0x") {
            i64::from_str_radix(h, 16)
        } else if t.len() > 1 && t.starts_with('0') {
            i64::from_str_radix(&t[1..], 8)
        } else {
            t.parse()
        };
        match parsed {
            Ok(i) => return Ok(Value::Int(if neg { -i } else { i })),
            // Psych makes a Bignum; it saturates here (docs/known-gaps.md).
            Err(e) if *e.kind() == std::num::IntErrorKind::PosOverflow => {
                return Ok(Value::Int(if neg { i64::MIN } else { i64::MAX }));
            }
            Err(_) => {}
        }
    }
    Ok(Value::Str(s.into()))
}

/// Whether `--show-cops` (Psych's YAMLTree) sees `s` load back as a String.
pub fn tokenizes_to_string(s: &str) -> bool {
    matches!(tokenize(s, true), Ok(Value::Str(_)))
}

/// String#to_i: a sign and digits, a `_` only between digits, up to the first other character.
fn to_i(n: &str) -> i128 {
    static RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[-+]?[0-9]+(?:_[0-9]+)*").unwrap());
    let t = RE.find(n).map_or("0", |m| m.as_str()).replace('_', "");
    // Too long for i128: saturated, like the sum.
    t.parse().unwrap_or(if t.starts_with('-') { i128::MIN } else { i128::MAX })
}

/// String#to_f on a base-60 part: `to_i`'s digits, then `.` and digits (with `_` between them).
fn to_f(n: &str) -> f64 {
    static RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^[-+]?[0-9]+(?:_[0-9]+)*(?:\.[0-9]+(?:_[0-9]+)*)?").unwrap());
    RE.find(n).map_or(0.0, |m| m.as_str().replace('_', "").parse().unwrap_or(0.0))
}

/// ScalarScanner#parse_time succeeds (`Time.utc` / `Time.new` take the parts): month 1-12, day
/// 1-31 (past the month's end rolls over), hour 0-24 (24 only at 24:00:00.x), minute 0-59, second
/// 0-60, offset under 24 hours.
fn valid_time(s: &str) -> bool {
    static PARTS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^-?[0-9]{4}-([0-9]{1,2})-([0-9]{1,2})").unwrap());
    static TIME: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"([0-9]+):([0-9]+):([0-9]+)(?:\.([0-9]*))?[ \t\r\n\x0B\x0C]*(Z|[-+][0-9]+(?::[0-9][0-9])?)?")
            .unwrap()
    });
    static TZ: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[-+]?([0-9]{1,2}):?([0-9]{1,2})?$").unwrap());
    // `string.split(/[ tT]/, 2)`.
    // ponytail: no ` `/`t`/`T` after the date (a tab) is a NoMethodError in Psych; a String here.
    let Some((date, time)) = s.split_once([' ', 't', 'T']) else { return false };
    let (Some(d), Some(t)) = (PARTS.captures(date), TIME.captures(time)) else { return false };
    let num = |m: Option<regex::Match>| m.map_or(0, |m| m.as_str().parse::<u64>().unwrap_or(u64::MAX));
    let (mon, day) = (num(d.get(1)), num(d.get(2)));
    let (hour, min, sec) = (num(t.get(1)), num(t.get(2)), num(t.get(3)));
    if !(1..=12).contains(&mon) || !(1..=31).contains(&day) || min > 59 || sec > 60 {
        return false;
    }
    if hour > 24 || (hour == 24 && (min > 0 || sec > 0)) {
        return false;
    }
    match t.get(5).map(|m| m.as_str()) {
        None | Some("Z") => true,
        Some(tz) => TZ.captures(tz).is_some_and(|c| {
            let (h, m) = (num(c.get(1)), num(c.get(2)));
            h * 3600 + m * 60 < 86400
        }),
    }
}

/// `Date.strptime(s, '%F', Date::GREGORIAN)` succeeds.
fn valid_date(s: &str) -> bool {
    let mut parts = s.split('-').map(|p| p.parse::<u32>().unwrap_or(0));
    let (y, m, d) = (parts.next().unwrap_or(0), parts.next().unwrap_or(0), parts.next().unwrap_or(0));
    let leap = y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
    let days = match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => 0,
    };
    (1..=days).contains(&d)
}

/// The Symbol name Psych's ScalarScanner gives a plain `:name` (`/^:(["'])(.*)\1/` takes the text
/// between the quotes of `:'name'` / `:"name"`, a leading `:` of it dropped).
fn symbol_name(v: &str) -> &str {
    let rest = &v[1..];
    let quoted = rest.chars().next().filter(|q| matches!(q, '\'' | '"')).and_then(|q| {
        let line = rest.split('\n').next().unwrap_or("");
        line[1..].rfind(q).map(|end| &line[1..1 + end])
    });
    match quoted {
        Some(name) => name.strip_prefix(':').unwrap_or(name),
        None => rest,
    }
}

/// Regexp#to_s of a `/src/flags` literal: `(?mi-x:src)`.
pub fn regexp_to_s(literal: &str) -> String {
    let (source, flags) = literal.strip_prefix('/').and_then(|r| r.rsplit_once('/')).unwrap_or((literal, ""));
    let on: String = "mix".chars().filter(|f| flags.contains(*f)).collect();
    let off: String = "mix".chars().filter(|f| !flags.contains(*f)).collect();
    format!("(?{on}{}{off}:{source})", if off.is_empty() { "" } else { "-" })
}

/// A Ruby regexp from the config. Patterns the regex crate accepts use it; those it rejects
/// (lookaround, backreferences, atomic groups, possessive quantifiers) fall back to fancy-regex's
/// backtracking engine.
///
/// fancy-regex gives up after a step budget where Onigmo (with its match cache) finishes.
/// A search that gives up is retried one start position at a time with the `\G`-anchored copy, so
/// a later match is still found; a start position that gives up on its own counts as no match
/// there (docs/known-gaps.md).
#[derive(Clone, Debug)]
pub enum RubyRegex {
    Plain(Regex),
    Backtracking { re: Box<fancy_regex::Regex>, anchored: Box<fancy_regex::Regex> },
}

// ponytail: fancy-regex has no match cache, so these budgets only bound the cost of exponential
// patterns: one search gets SEARCH_LIMIT steps, each retried start position ANCHORED_LIMIT, and a
// whole find_at / replace_all call RETRY_BUDGET for the searches that give up and the retries
// (one search, then 64 positions). An anchored retry also costs time linear in the text (the
// delegated inner regex), so the retry count is kept small: about 1 s on a 100k-character line.
const SEARCH_LIMIT: usize = 1_000_000;
const ANCHORED_LIMIT: usize = 10_000;
const RETRY_BUDGET: usize = SEARCH_LIMIT + 64 * ANCHORED_LIMIT;
// A pattern that gives up on one input gives up on the next, and a cop calls it once per comment,
// token or identifier: this caps what one pattern spends giving up on one file (about 2 s), after
// which it answers "no match" at once. Keyed by pattern: cops build a RubyRegex per call from their
// config. Per file (`reset_run_budget`, on the thread linting it), so a file's offenses never depend
// on the other files of the run or on which worker thread got there first (#324).
const RUN_BUDGET: usize = 32 * RETRY_BUDGET;
thread_local! {
    static SPENT: std::cell::RefCell<Vec<(String, usize)>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Start a new file's RUN_BUDGET for every pattern (this thread).
pub fn reset_run_budget() {
    SPENT.with(|s| s.borrow_mut().clear());
}

/// Charge `steps` to the pattern's total for the current file; false once it is over RUN_BUDGET.
fn charge(pattern: &str, steps: usize) -> bool {
    SPENT.with(|s| charge_in(&mut s.borrow_mut(), pattern, steps))
}

fn charge_in(spent: &mut Vec<(String, usize)>, pattern: &str, steps: usize) -> bool {
    let total = match spent.iter_mut().find(|(p, _)| p == pattern) {
        Some((_, t)) => t,
        None => {
            spent.push((pattern.to_string(), 0));
            &mut spent.last_mut().unwrap().1
        }
    };
    *total = total.saturating_add(steps);
    *total < RUN_BUDGET
}

fn fancy(pattern: &str, limit: usize) -> Option<Box<fancy_regex::Regex>> {
    fancy_regex::RegexBuilder::new(pattern).backtrack_limit(limit).build().ok().map(Box::new)
}

impl RubyRegex {
    pub fn is_match(&self, text: &str) -> bool {
        self.find_at(text, 0).is_some()
    }

    /// The compiled pattern (Ruby classes and flags already translated).
    pub fn as_str(&self) -> &str {
        match self {
            RubyRegex::Plain(r) => r.as_str(),
            RubyRegex::Backtracking { re, .. } => re.as_str(),
        }
    }

    /// The leftmost match starting at or after byte `start`, as a byte range.
    pub fn find_at(&self, text: &str, start: usize) -> Option<(usize, usize)> {
        let mut budget = RETRY_BUDGET;
        self.find_budgeted(text, start, &mut budget)
    }

    /// `find_at` charging `budget` for every search that gives up and every anchored retry (at
    /// its full step limit); once it is spent the rest of the text counts as no match.
    fn find_budgeted(&self, text: &str, start: usize, budget: &mut usize) -> Option<(usize, usize)> {
        let (re, anchored) = match self {
            RubyRegex::Plain(r) => return r.find_at(text, start).map(|m| (m.start(), m.end())),
            RubyRegex::Backtracking { re, anchored } => (re, anchored),
        };
        if *budget < SEARCH_LIMIT || !charge(re.as_str(), 0) {
            return None;
        }
        match re.find_from_pos(text, start) {
            Ok(m) => return m.map(|m| (m.start(), m.end())),
            Err(_) => *budget -= SEARCH_LIMIT,
        }
        if !charge(re.as_str(), SEARCH_LIMIT) {
            return None;
        }
        for p in (start..=text.len()).filter(|&p| text.is_char_boundary(p)) {
            if *budget < ANCHORED_LIMIT || !charge(re.as_str(), ANCHORED_LIMIT) {
                return None;
            }
            *budget -= ANCHORED_LIMIT;
            if let Ok(Some(m)) = anchored.find_from_pos(text, p) {
                return Some((m.start(), m.end()));
            }
        }
        None
    }

    /// `String#gsub` with a block: each non-overlapping match replaced by `f(matched text)`.
    pub fn replace_all_with<'t>(&self, text: &'t str, mut f: impl FnMut(&str) -> String) -> Cow<'t, str> {
        let mut out = String::new();
        let (mut last, mut pos, mut any) = (0, 0, false);
        let mut budget = RETRY_BUDGET;
        while pos <= text.len() {
            let Some((s, e)) = self.find_budgeted(text, pos, &mut budget) else { break };
            out.push_str(&text[last..s]);
            out.push_str(&f(&text[s..e]));
            (last, any) = (e, true);
            pos = if e > s { e } else { e + text[e..].chars().next().map_or(1, char::len_utf8) };
        }
        if !any {
            return Cow::Borrowed(text);
        }
        out.push_str(&text[last..]);
        Cow::Owned(out)
    }

    /// `String#gsub(re, rep)` with a literal replacement.
    pub fn replace_all<'t>(&self, text: &'t str, rep: &str) -> Cow<'t, str> {
        self.replace_all_with(text, |_| rep.to_string())
    }
}

/// Compile a Ruby regexp literal (`/src/flags`) or bare source.
// ponytail: syntax neither engine knows (`\h` inside classes, `(?<name>` quirks, `\G`, `\K`)
// fails to compile and the pattern is skipped.
pub fn ruby_regex(literal: &str) -> Option<RubyRegex> {
    // Cops compile their config's patterns per file (or per node); compiling is far slower than
    // matching, so each thread keeps what it compiled. Clones share the compiled program.
    // ponytail: cleared when full; patterns come from config, so it never fills in practice.
    thread_local! {
        static COMPILED: std::cell::RefCell<std::collections::HashMap<String, Option<RubyRegex>>> =
            std::cell::RefCell::new(std::collections::HashMap::new());
    }
    if let Some(hit) = COMPILED.with(|c| c.borrow().get(literal).cloned()) {
        return hit;
    }
    let re = compile_ruby_regex(literal);
    COMPILED.with(|c| {
        let mut c = c.borrow_mut();
        if c.len() >= 4096 {
            c.clear();
        }
        c.insert(literal.to_string(), re.clone());
    });
    re
}

fn compile_ruby_regex(literal: &str) -> Option<RubyRegex> {
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
    let pattern = format!("(?{prefix}){}", crate::helpers::ruby_string::ruby_regex_classes(source));
    match Regex::new(&pattern) {
        Ok(r) => Some(RubyRegex::Plain(r)),
        Err(_) => Some(RubyRegex::Backtracking {
            re: fancy(&pattern, SEARCH_LIMIT)?,
            anchored: fancy(&format!(r"\G(?:{pattern})"), ANCHORED_LIMIT)?,
        }),
    }
}

#[cfg(test)]
mod integer_tests {
    // Ruby 4.0.6 `Integer(s)` (nil: ArgumentError)
    #[test]
    fn ruby_integer_strings() {
        let cases: &[(&str, Option<i64>)] = &[
            ("3", Some(3)),
            (" 3 ", Some(3)),
            ("\t3\n", Some(3)),
            ("0x1A", Some(26)),
            ("0X1a", Some(26)),
            ("0b11", Some(3)),
            ("0o7", Some(7)),
            ("07", Some(7)),
            ("08", None),
            ("1_000", Some(1000)),
            ("1__0", None),
            ("_1", None),
            ("+3", Some(3)),
            ("-3", Some(-3)),
            ("+-3", None),
            ("- 3", None),
            ("3.0", None),
            (" ", None),
            ("", None),
            ("3e2", None),
            ("0_7", Some(7)),
            ("0x", None),
            ("1_", None),
            ("0d9", Some(9)),
            ("0_x1", None),
            ("0x_1", None),
            ("3\0", None),
            ("\u{ff13}", None),
        ];
        for (s, want) in cases {
            assert_eq!(super::ruby_integer(s), *want, "{s:?}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Onigmo: `"a"*46 + "B" =~ /(a|aa)+(?=c)|B/` is 46; gsub leaves the text when nothing matches.
    #[test]
    fn backtracking_regex_past_the_step_budget() {
        let text = format!("{}B", "a".repeat(46));
        let re = ruby_regex("(a|aa)+(?=c)|B").unwrap();
        assert!(matches!(re, RubyRegex::Backtracking { .. }));
        assert_eq!(re.find_at(&text, 0), Some((46, 47)));
        let re = ruby_regex("(a|aa)+(?=c)").unwrap();
        assert!(!re.is_match(&text));
        assert_eq!(re.replace_all(&text, ""), text);
        let re = ruby_regex("/a(?=b)/i").unwrap();
        assert_eq!(re.replace_all_with("xAbab", |m| format!("<{m}>")), "x<A>b<a>b");
        // One budget per call: a 100k-character line costs a few million steps, not len * limit.
        let long = "a".repeat(100_000);
        let re = ruby_regex("(a|aa)+(?=c)").unwrap();
        let t0 = std::time::Instant::now();
        assert!(!re.is_match(&long));
        assert_eq!(re.replace_all(&long, ""), long);
        assert!(t0.elapsed() < std::time::Duration::from_secs(20), "took {:?}", t0.elapsed());
    }

    // A pattern that gives up on every call stops being searched once it has spent RUN_BUDGET:
    // 200 calls cost 32 budgets, not 200 (about 70 ms each on a 60-character line). No wall-clock
    // assert: it failed on a loaded machine; the exhausted-pattern checks below prove the budget.
    #[test]
    fn backtracking_regex_past_the_run_budget() {
        let re = ruby_regex("(a|aa)+(?=d)|B").unwrap();
        let line = format!("# {} 1", "a".repeat(60));
        for _ in 0..200 {
            assert!(!re.is_match(&line));
        }
        // The ceiling: a cheap match of the exhausted pattern is no longer found.
        assert!(!re.is_match("B"));
        // Until the next file.
        reset_run_budget();
        assert!(re.is_match("B"));
    }

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

    // Ruby 4.0 Psych (`YAML.safe_load("k: <s>")`): YAML 1.1 booleans, `_`/`,` in numbers, leading-0
    // octal, base 60, and what stays a String.
    #[test]
    fn plain_scalars_resolve_like_psych() {
        use Value::*;
        let cases: &[(&str, Value)] = &[
            ("yes", Bool(true)),
            ("Yes", Bool(true)),
            ("On", Bool(true)),
            ("no", Bool(false)),
            ("OFF", Bool(false)),
            ("y", Str("y".into())),
            ("N", Str("N".into())),
            ("False", Bool(false)),
            ("~", Null),
            ("NULL", Null),
            ("nil", Str("nil".into())),
            ("yess", Str("yess".into())),
            ("1_000", Int(1000)),
            ("1__0", Str("1__0".into())),
            ("0100", Int(64)),
            ("08", Str("08".into())),
            ("0o17", Str("0o17".into())),
            ("0b101", Int(5)),
            ("0x1F", Int(31)),
            ("-0x1f", Int(-31)),
            ("+12", Int(12)),
            ("1,000", Int(1000)),
            ("1:30", Int(5400)),
            ("-1:30", Int(-1800)),
            ("1:2:3", Int(3723)),
            ("1:30.5", Float(5430.0)),
            ("1.", Float(1.0)),
            (".5", Float(0.5)),
            ("+.5", Float(0.5)),
            ("1_0.5", Float(10.5)),
            ("1.0e+3", Float(1000.0)),
            ("1.0e3", Str("1.0e3".into())),
            ("1e3", Str("1e3".into())),
            (".", Str(".".into())),
            ("-.", Str("-.".into())),
            ("-.Inf", Float(f64::NEG_INFINITY)),
            ("00", Int(0)),
            ("-0", Int(0)),
            ("_1", Str("_1".into())),
            ("12abc", Str("12abc".into())),
            ("1.2.3", Str("1.2.3".into())),
            ("9223372036854775807", Int(i64::MAX)),
        ];
        for (src, want) in cases {
            let Value::Map(m) = parse_yaml(&format!("k: {src}\n")).unwrap() else { panic!("not a map") };
            assert_eq!(m.get("k"), Some(want), "{src}");
        }
    }
    // Psych's dump-side ScalarScanner (#594): a date or time Ruby rejects stays a String, so
    // YAMLTree writes it plain.
    #[test]
    fn invalid_dates_and_times_tokenize_to_strings() {
        for s in ["2001-02-30", "2001-00-00", "1900-02-29", "2001-13-45 10:00:00", "2001-01-01 25:00:00"] {
            assert!(tokenizes_to_string(s), "{s}");
        }
        for s in ["2000-02-29", "2001-1-1", "2001-02-31 10:00:00", "2001-01-01 24:00:00.5", "2001-01-01 1:00:00 +23:59"]
        {
            assert!(!tokenizes_to_string(s), "{s}");
        }
        assert!(tokenizes_to_string("2001-01-01 10:00:00 +24"));
        assert_eq!(parse_yaml("k: 2001-02-30\n"), Err("Tried to load unspecified class: Date".into()));
        let Ok(Value::Map(m)) = unsafe_load("k: 2001-01-01\nj: 2001-02-30\n") else { panic!() };
        assert_eq!((m.get("k"), m.str("j")), (Some(&Value::Null), Some("2001-02-30")));
    }

    // ScalarScanner's base 60 (#595): String#to_i / #to_f per part, Integer arithmetic.
    #[test]
    fn base_60_parts_as_ruby_reads_them() {
        for (src, want) in [
            ("1__0:00", Value::Int(3600)),
            ("1_0:00", Value::Int(36000)),
            ("1:30._5", Value::Float(5400.0)),
            ("1:30.5_5", Value::Float(3600.0 + 30.55 * 60.0)),
            ("2000000000000001:00", Value::Int(7200000000000003600)),
            ("-1:30", Value::Int(-1800)),
        ] {
            let Value::Map(m) = parse_yaml(&format!("k: {src}\n")).unwrap() else { panic!("not a map") };
            assert_eq!(m.get("k"), Some(&want), "{src}");
        }
    }

    // libyaml's reader decodes UTF-8 itself (#593): its problems, and a bad byte in a 16 KB chunk
    // never read losing to an earlier scanner error.
    #[test]
    fn utf8_reader_errors() {
        let err = |b: &[u8]| parse_yaml_bytes(b, "f").unwrap_err();
        assert_eq!(err(b"# \xff\n"), "(f): invalid leading UTF-8 octet at line 1 column 1");
        assert_eq!(err(b"\xff\xfea\x00"), "(f): invalid leading UTF-8 octet at line 1 column 1");
        assert_eq!(err(b"Foo: a\xe2\x82\n"), "(f): invalid trailing UTF-8 octet at line 1 column 1");
        assert_eq!(err(b"# a\xe2"), "(f): incomplete UTF-8 octet sequence at line 1 column 1");
        assert_eq!(err(b"a: \xc0\xaf"), "(f): invalid length of a UTF-8 sequence at line 1 column 1");
        assert_eq!(err(b"a: \xed\xa0\x80"), "(f): invalid Unicode character at line 1 column 1");
        assert_eq!(err(b"a: \xf4\x90\x80\x80"), "(f): invalid Unicode character at line 1 column 1");
        let chunked = |n: usize| [b"a: b: c\n# ".as_slice(), &vec![b'x'; n], b"\xff\n"].concat();
        assert_eq!(err(&chunked(1000)), "(f): invalid leading UTF-8 octet at line 1 column 1");
        assert_eq!(err(&chunked(20000)), "(f): mapping values are not allowed in this context at line 1 column 5");
    }
}
