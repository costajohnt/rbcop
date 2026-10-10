//! Config values as RuboCop sees them after Psych: insertion-ordered maps, plus Regexp.

use regex::Regex;
use std::sync::{Arc, LazyLock};
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
    parse_yaml_file(text, "<unknown>")
}

/// `parse_yaml`, with errors worded as RuboCop prints them for the config file `path`: Psych's
/// `(path): <problem> <context> at line L column C`, `An alias referenced an unknown anchor: x`,
/// `Tried to load unspecified class: Date`.
pub fn parse_yaml_file(text: &str, path: &str) -> Result<Value, String> {
    let text = mark_regexps(text);
    let mut checks = Checks { chars: text.chars().collect(), ..Checks::default() };
    if let Err(e) = Parser::new(text.chars()).load(&mut checks, true) {
        return Err(checks.syntax_error(&e, path));
    }
    if let Some(e) = checks.error {
        return Err(e);
    }
    Ok(checks.loader.docs.into_iter().next().unwrap_or_else(|| Value::Map(Default::default())))
}

/// Feeds the loader, keeping what Psych's error messages need: the open collections (where
/// libyaml's context mark is, flow or block) and the first class the restricted loader refuses.
#[derive(Default)]
struct Checks {
    loader: Loader,
    chars: Vec<char>,
    /// Start mark of each open collection, and whether it is a flow collection.
    opens: Vec<(Marker, bool)>,
    /// A block mapping just opened: yaml-rust2 marks it at the first `:`, libyaml at its first key,
    /// the next event.
    key_mark_pending: bool,
    error: Option<String>,
}

impl MarkedEventReceiver for Checks {
    fn on_event(&mut self, ev: Event, mark: Marker) {
        if std::mem::take(&mut self.key_mark_pending)
            && let Some(open) = self.opens.last_mut()
        {
            open.0 = mark;
        }
        let refused = match &ev {
            Event::SequenceStart(_, tag) | Event::MappingStart(_, tag) => {
                let flow = matches!(self.chars.get(mark.index()), Some('[' | '{'));
                self.opens.push((mark, flow));
                self.key_mark_pending = !flow && matches!(ev, Event::MappingStart(..));
                tag.as_ref().and_then(refused_tag)
            }
            Event::SequenceEnd | Event::MappingEnd => {
                self.opens.pop();
                None
            }
            Event::Scalar(v, style, _, tag) => match tag {
                Some(t) => refused_tag(t),
                None if *style == TScalarStyle::Plain => refused_scalar(v),
                None => None,
            },
            _ => None,
        };
        if self.error.is_none()
            && let Some(class) = refused
        {
            self.error = Some(format!("Tried to load unspecified class: {class}"));
        }
        self.loader.on_event(ev, mark);
    }
}

impl Checks {
    /// Psych::SyntaxError#message (libyaml's problem, then its context, at the context mark), or
    /// Psych's AnchorNotDefined for an alias yaml-rust2 rejects.
    fn syntax_error(&self, e: &yaml_rust2::ScanError, path: &str) -> String {
        let info = e.info();
        if info == "while parsing node, found unknown anchor" {
            let name: String = self.chars[(e.marker().index() + 1).min(self.chars.len())..]
                .iter()
                .take_while(|c| !c.is_whitespace() && !matches!(c, ',' | '[' | ']' | '{' | '}'))
                .collect();
            return format!("An alias referenced an unknown anchor: {name}");
        }
        let (problem, context) = match info.split_once(", ") {
            Some((c, p)) if c.starts_with("while ") => (p, Some(c)),
            _ => (info, None),
        };
        let at = self.chars.get(e.marker().index()).copied();
        let (problem, context) = match (problem, context) {
            ("expected ',' or ']'", c) => ("did not find expected ',' or ']'", c),
            (p, Some("while scanning an anchor or alias")) => {
                (p, Some(if at == Some('*') { "while scanning an alias" } else { "while scanning an anchor" }))
            }
            (p, None) if p.starts_with("unexpected character: ") || p.starts_with("tabs disallowed") => {
                ("found character that cannot start any token", Some("while scanning for the next token"))
            }
            pc => pc,
        };
        let in_flow = self.opens.last().is_some_and(|o| o.1);
        let context = match context {
            Some("while parsing a node") if in_flow => Some("while parsing a flow node"),
            Some("while parsing a node") => Some("while parsing a block node"),
            c => c,
        };
        // libyaml marks a collection error at the collection's start.
        let mark = match context {
            Some(
                "while parsing a block mapping"
                | "while parsing a flow mapping"
                | "while parsing a flow sequence"
                | "while parsing a block collection",
            ) => self.opens.last().map_or(*e.marker(), |o| o.0),
            _ => *e.marker(),
        };
        let what = context.map_or(problem.to_string(), |c| format!("{problem} {c}"));
        // yaml-rust2 marks a disallowed tab after it, libyaml at it.
        let col = if info.starts_with("tabs disallowed") { mark.col() } else { mark.col() + 1 };
        format!("({path}): {what} at line {} column {col}", mark.line())
    }
}

/// The class Psych's restricted loader (RuboCop permits Regexp and Symbol) refuses for a tag.
fn refused_tag(tag: &yaml_rust2::parser::Tag) -> Option<String> {
    let suffix = tag.suffix.strip_prefix("ruby/")?;
    ["object:", "struct:", "hash:", "array:", "exception:"]
        .iter()
        .find_map(|p| suffix.strip_prefix(p))
        .map(String::from)
}

/// ScalarScanner turns a plain timestamp into a Time and a date into a Date, which RuboCop's
/// restricted loader refuses.
fn refused_scalar(s: &str) -> Option<String> {
    static TIME: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(
            r"^-?\d{4}-\d{1,2}-\d{1,2}(?:[Tt]|[ \t\r\n\x0B\x0C]+)\d{1,2}:\d\d:\d\d(?:\.\d*)?(?:[ \t\r\n\x0B\x0C]*(?:Z|[-+]\d{1,2}:?(?:\d\d)?))?$",
        )
        .unwrap()
    });
    static DATE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^\d{4}-(?:1[012]|0\d|\d)-(?:[12]\d|3[01]|0\d|\d)$").unwrap());
    if TIME.is_match(s) {
        Some("Time".into())
    } else if DATE.is_match(s) {
        Some("Date".into())
    } else {
        None
    }
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
                        _ => psych_scalar(&v),
                    },
                    (TScalarStyle::Plain, Some(_)) => Yaml::String(v),
                    (TScalarStyle::Plain, None) => psych_scalar(&v),
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

/// Psych's `ScalarScanner#tokenize` (YAML 1.1, `strict_integer: false`) for a plain scalar:
/// `yes`/`on`/`off`, `1_000`, `0100` (octal), `1:30` (base 60), `1.` and so on.
// ponytail: timestamps and dates stay strings (RuboCop's safe_load refuses Time/Date anyway); an
// integer beyond i64 stays a string.
fn psych_scalar(s: &str) -> Yaml {
    static WORDISH: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^[^\d.:-]?[\p{Alphabetic}_ \t\r\n\x0B\x0C!@#$%^&*(){}<>|/\\~;=]+").unwrap());
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
    let lower = s.to_ascii_lowercase();
    if s.is_empty() {
        return Yaml::Null;
    }
    if WORDISH.is_match(s) || s.contains('\n') {
        return match lower.as_str() {
            _ if s.chars().count() > 5 => Yaml::String(s.into()),
            "~" | "null" => Yaml::Null,
            "yes" | "true" | "on" => Yaml::Boolean(true),
            "no" | "false" | "off" => Yaml::Boolean(false),
            _ => Yaml::String(s.into()),
        };
    }
    match lower.as_str() {
        ".inf" | "+.inf" => return Yaml::Real(".inf".into()),
        "-.inf" => return Yaml::Real("-.inf".into()),
        ".nan" => return Yaml::Real(".nan".into()),
        _ => {}
    }
    // `n.to_i * 60 ** (e - 2).abs` per part, as Psych writes it (so `1:30` is 5400).
    let base60 = |to_num: &dyn Fn(&str) -> f64| -> f64 {
        s.split(':').enumerate().map(|(e, n)| to_num(n) * 60f64.powi((e as i32 - 2).abs())).sum()
    };
    let digits = |n: &str| n.replace('_', "");
    if SEXA_INT.is_match(s) {
        return Yaml::Integer(base60(&|n| digits(n).parse::<i64>().unwrap_or(0) as f64) as i64);
    }
    if SEXA_FLOAT.is_match(s) {
        return Yaml::Real(base60(&|n| digits(n).parse::<f64>().unwrap_or(0.0)).to_string());
    }
    if FLOAT.is_match(s) {
        let t = s.replace([',', '_'], "");
        let t = t.replacen(".e", "e", 1).replacen(".E", "E", 1);
        let t = t.strip_suffix('.').unwrap_or(&t);
        return match t.parse::<f64>() {
            Ok(f) if !matches!(t, "" | "+" | "-") => Yaml::Real(f.to_string()),
            _ => Yaml::String(s.into()),
        };
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
        if let Ok(i) = parsed {
            return Yaml::Integer(if neg { -i } else { i });
        }
    }
    Yaml::String(s.into())
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
#[derive(Clone, Debug)]
pub enum RubyRegex {
    Plain(Regex),
    Backtracking(Box<fancy_regex::Regex>),
}

impl RubyRegex {
    /// A backtracking match that hits fancy-regex's backtrack limit counts as no match.
    pub fn is_match(&self, text: &str) -> bool {
        match self {
            RubyRegex::Plain(r) => r.is_match(text),
            RubyRegex::Backtracking(r) => r.is_match(text).unwrap_or(false),
        }
    }

    /// The compiled pattern (Ruby classes and flags already translated).
    pub fn as_str(&self) -> &str {
        match self {
            RubyRegex::Plain(r) => r.as_str(),
            RubyRegex::Backtracking(r) => r.as_str(),
        }
    }

    pub fn replace_all<'t>(&self, text: &'t str, rep: &str) -> std::borrow::Cow<'t, str> {
        match self {
            RubyRegex::Plain(r) => r.replace_all(text, rep),
            RubyRegex::Backtracking(r) => r.replace_all(text, rep),
        }
    }
}

/// Compile a Ruby regexp literal (`/src/flags`) or bare source.
// ponytail: syntax neither engine knows (`\h` inside classes, `(?<name>` quirks, `\G`, `\K`)
// fails to compile and the pattern is skipped.
pub fn ruby_regex(literal: &str) -> Option<RubyRegex> {
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
        Err(_) => fancy_regex::Regex::new(&pattern).ok().map(|r| RubyRegex::Backtracking(Box::new(r))),
    }
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
}
