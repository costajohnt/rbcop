use crate::helpers::parser_tokens::{self as pt, T_STRING, T_SYMBOL};
use crate::helpers::ruby_string::{ruby_regex_classes, strip};
use crate::prelude::*;
use std::sync::{Arc, Mutex};

pub const NAME: &str = "Naming/InclusiveLanguage";
pub static COP: InclusiveLanguage = InclusiveLanguage;
pub struct InclusiveLanguage;

/// One `FlaggedTerms` entry: its regex (WholeWord's lookarounds become a boundary check), the
/// message suffix and the sole suggestion used by the corrector.
struct Term {
    re: Regex,
    whole_word: bool,
    suffix: String,
    sole: Option<String>,
}

struct Compiled {
    terms: Vec<Term>,
    allowed: Option<Regex>,
    checks: [bool; 6],
}

const CHECKS: [&str; 6] =
    ["CheckIdentifiers", "CheckConstants", "CheckVariables", "CheckSymbols", "CheckStrings", "CheckComments"];

// ponytail: one cache slot per distinct config, keyed by its debug print; configs are few.
static CACHE: Mutex<Vec<(String, Arc<Compiled>)>> = Mutex::new(Vec::new());

/// `Regexp#source` of a config value: a `!ruby/regexp` literal (`/src/flags`, plain or quoted in
/// YAML), or a string used verbatim as a regex source.
fn regex_source(v: &Value) -> Option<String> {
    match v {
        Value::Regex(r) => {
            let src = r.strip_prefix('/').and_then(|r| r.rsplit_once('/')).map_or(r.as_str(), |(src, _)| src);
            Some(src.to_string())
        }
        v => v.as_string(),
    }
}

/// `Regexp.new(source, Regexp::IGNORECASE)`; Ruby's `^`/`$` are line anchors.
fn compile(source: &str) -> Option<Regex> {
    Regex::new(&format!("(?im){}", ruby_regex_classes(source))).ok()
}

/// preprocess_suggestions + format_suggestions.
fn suffix(v: Option<&Value>) -> String {
    let words: Vec<String> = match v {
        None | Some(Value::Null) => return String::new(),
        Some(Value::Seq(items)) => items.iter().filter_map(Value::as_string).collect(),
        Some(v) => v.as_string().into_iter().filter(|s| !strip(s).is_empty()).collect(),
    };
    let mut q: Vec<String> = words.iter().map(|w| format!("'{w}'")).collect();
    let s = match q.len() {
        0 => return String::new(),
        1 => q.remove(0),
        2 => q.join(" or "),
        _ => {
            let last = q.pop().unwrap();
            q.push(format!("or {last}"));
            q.join(", ")
        }
    };
    format!(" with {s}")
}

/// preferred_sole_term.
fn sole(v: Option<&Value>) -> Option<String> {
    match v? {
        Value::Seq(items) if items.len() == 1 => sole(items.first()),
        Value::Str(s) => Some(s.clone()),
        _ => None,
    }
}

fn compiled(cx: &Cx) -> Arc<Compiled> {
    let key = format!("{:?}{:?}", cx.cfg.get("FlaggedTerms"), CHECKS.map(|k| cx.cfg.get(k)));
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((_, c)) = cache.iter().find(|(k, _)| *k == key) {
        return c.clone();
    }
    let mut terms = Vec::new();
    let mut allowed = Vec::new();
    if let Some(Value::Map(map)) = cx.cfg.get("FlaggedTerms") {
        for (term, def) in &map.0 {
            let Value::Map(def) = def else { continue };
            match def.get("AllowedRegex") {
                Some(Value::Seq(items)) => allowed.extend(items.iter().filter_map(regex_source)),
                Some(Value::Null) | None => {}
                Some(v) => allowed.extend(regex_source(v)),
            }
            let (source, whole_word) = if def.truthy("Regex") {
                (def.get("Regex").and_then(regex_source).unwrap_or_default(), false)
            } else {
                (term.clone(), def.truthy("WholeWord"))
            };
            // ponytail: a pattern the regex crate cannot compile (lookaround, backrefs) drops the term.
            let Some(re) = compile(&source) else { continue };
            terms.push(Term {
                re,
                whole_word,
                suffix: suffix(def.get("Suggestions")),
                sole: sole(def.get("Suggestions")),
            });
        }
    }
    // process_allowed_regex skips blank strings.
    allowed.retain(|s| !strip(s).is_empty());
    let allowed = if allowed.is_empty() { None } else { compile(&allowed.join("|")) };
    let checks = CHECKS.map(|k| cx.cfg.bool(k).unwrap_or(false));
    let c = Arc::new(Compiled { terms, allowed, checks });
    cache.push((key, c.clone()));
    c
}

fn ascii_alnum(c: Option<char>) -> bool {
    c.is_some_and(|c| c.is_ascii_alphanumeric())
}

fn word_char(c: Option<char>) -> bool {
    c.is_some_and(|c| c.is_alphanumeric() || c == '_')
}

/// `(?:\b|(?<=[\W_]))term(?:\b|(?=[\W_]))`: Ruby's `\W` is ASCII, its `\b` Unicode.
fn whole_word_ok(s: &str, start: usize, end: usize) -> bool {
    let m = &s[start..end];
    let prev = s[..start].chars().next_back();
    let next = s[end..].chars().next();
    let before = !ascii_alnum(prev) || word_char(prev) != word_char(m.chars().next());
    let after = !ascii_alnum(next) || word_char(next) != word_char(m.chars().next_back());
    before && after
}

fn char_len_at(s: &str, i: usize) -> usize {
    s[i..].chars().next().map_or(1, char::len_utf8)
}

/// First acceptable match of a term at or after `from`.
fn find_term(t: &Term, s: &str, mut from: usize) -> Option<(usize, usize)> {
    while from <= s.len() {
        let m = t.re.find_at(s, from)?;
        if !t.whole_word || whole_word_ok(s, m.start(), m.end()) {
            return Some((m.start(), m.end()));
        }
        from = m.start() + char_len_at(s, m.start());
    }
    None
}

impl Compiled {
    /// mask_input + `String#scan` with the terms joined by `|`: leftmost match, earliest term on
    /// a tie. Each term's next match is cached, so the work stays linear in the input.
    fn scan(&self, input: &str) -> Vec<String> {
        let masked;
        let s = match &self.allowed {
            Some(a) => {
                masked = a.replace_all(input, |c: &regex::Captures| "*".repeat(c[0].chars().count()));
                &*masked
            }
            None => input,
        };
        let mut next: Vec<Option<(usize, usize)>> = self.terms.iter().map(|t| find_term(t, s, 0)).collect();
        let mut out = Vec::new();
        let mut pos = 0;
        while pos <= s.len() {
            for (i, t) in self.terms.iter().enumerate() {
                if next[i].is_some_and(|(st, _)| st < pos) {
                    next[i] = find_term(t, s, pos);
                }
            }
            let Some((st, en)) = next.iter().flatten().copied().min_by_key(|&(st, _)| st) else { break };
            out.push(s[st..en].to_string());
            pos = if en > st { en } else { en + char_len_at(s, en) };
        }
        out
    }

    /// find_flagged_term: the first term whose regex matches the word on its own.
    fn term_for(&self, word: &str) -> Option<&Term> {
        self.terms.iter().find(|t| find_term(t, word, 0).is_some())
    }
}

/// create_message (`where_` is "" or " in file path").
fn message(term: Option<&Term>, word: &str, where_: &str) -> String {
    let suffix = term.map_or("", |t| t.suffix.as_str());
    let suffix = if suffix.is_empty() { " with another term" } else { suffix };
    format!("Consider replacing '{word}'{where_}{suffix}.")
}

impl Cop for InclusiveLanguage {
    fn correctable(&self) -> bool {
        true
    }

    fn on_file(&self, cx: &mut Cx) {
        let c = compiled(cx);
        if c.terms.is_empty() {
            return;
        }
        if cx.cfg.bool("CheckFilepaths").unwrap_or(false) {
            let words = c.scan(cx.src.path);
            match words.len() {
                0 => {}
                1 => cx.add_global(message(c.term_for(&words[0]), &words[0], " in file path")),
                _ => cx.add_global(format!(
                    "Consider replacing '{}' in file path with other terms.",
                    words.join("', '")
                )),
            }
        }
        let [ident, constant, var, sym, string, comment] = c.checks;
        let src = cx.src;
        let mut prev = 0;
        for t in pt::translated(src) {
            let after_def = std::mem::replace(&mut prev, t.kind) == sys::PM_TOKEN_KEYWORD_DEF;
            let checked = match t.kind {
                sys::PM_TOKEN_IDENTIFIER => ident,
                // The translation lexer makes `def foo?`'s tFID a tIDENTIFIER.
                sys::PM_TOKEN_METHOD_NAME => ident && after_def,
                sys::PM_TOKEN_CONSTANT => constant,
                sys::PM_TOKEN_INSTANCE_VARIABLE | sys::PM_TOKEN_CLASS_VARIABLE | sys::PM_TOKEN_GLOBAL_VARIABLE => var,
                T_SYMBOL => sym,
                T_STRING | sys::PM_TOKEN_STRING_CONTENT => string,
                sys::PM_TOKEN_COMMENT => comment,
                _ => false,
            };
            if !checked {
                continue;
            }
            let source = src.slice(t.start, t.end);
            // token.text: a symbol without its colon, a simple string without its quotes.
            // ponytail: escapes are scanned as written, not unescaped.
            let text = match t.kind {
                T_SYMBOL => source.get(1..).unwrap_or(""),
                T_STRING => source.get(1..source.len().saturating_sub(1)).unwrap_or(""),
                _ => source,
            };
            for word in c.scan(text) {
                // offense_range: the first occurrence of the word in the token's source.
                let Some(i) = source.find(&word) else { continue };
                let (start, end) = (t.start + i, t.start + i + word.len());
                let term = c.term_for(&word);
                let sole = term.and_then(|t| t.sole.clone());
                cx.add_fix(start, end, message(term, &word, ""), |corr| {
                    if let Some(s) = sole {
                        corr.replace(start, end, &s);
                    }
                });
            }
        }
    }
}
