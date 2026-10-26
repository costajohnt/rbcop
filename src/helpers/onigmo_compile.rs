//! Onigmo compile errors (`Regexp.new`) for a static regexp literal that Prism's regexp checker
//! does not report. The parser gem's builder (`static_regexp`) and the Prism translation both
//! compile every static regexp, so RuboCop reports these as Lint/Syntax at every target.
//!
//! Checked: group names and references (`\k<..>`, `\g<..>`, `\1`), repeat ranges (`{3,2}`,
//! `{100001}`), character class ranges and POSIX bracket names, and look-behind bodies that are not
//! fixed-length. Messages and their order are Onigmo's (Ruby 4.0): parse errors left to right,
//! then reference errors, then look-behind.
//!
//! ponytail: anything this scanner does not model (extended mode, conditionals, `\x{..}`,
//! `\c`/`\M-`/`\C-` escapes, multi-digit backrefs, `\p{..}` names, alternation inside a
//! look-behind's groups) is treated as valid, so the check never invents an error; the gaps are in
//! docs/known-gaps.md.

/// Onigmo's error message for `source` (without the `: /source/` suffix), if it has one this
/// scanner knows. `None` also when the pattern uses something it does not model.
pub fn error(source: &str) -> Option<String> {
    let mut s = Scan { c: source.chars().collect(), i: 0, ..Scan::default() };
    match s.alternation(false) {
        Err(Stop::Error(m)) => return Some(m),
        Err(Stop::Unknown) => return None,
        Ok(()) => {}
    }
    if s.i < s.c.len() {
        // An unbalanced `)`: Prism reports it.
        return None;
    }
    let numbered = s.refs.iter().any(|r| matches!(r, Ref::Num(..) | Ref::Rel(..) | Ref::CallNum(..)));
    if !s.names.is_empty() && numbered {
        return Some("numbered backref/call is not allowed. (use name)".into());
    }
    for r in &s.refs {
        match r {
            Ref::Num(n) if *n > s.groups => return Some("invalid backref number/name".into()),
            Ref::Rel(k, before) if *k > *before => return Some("invalid backref number/name".into()),
            Ref::CallNum(n) if *n > s.groups => return Some(format!("undefined group <{n}> reference")),
            Ref::CallName(name) if !s.names.contains(name) => return Some(format!("undefined name <{name}> reference")),
            _ => {}
        }
    }
    s.bad_lookbehind.then(|| "invalid pattern in look-behind".into())
}

enum Stop {
    Error(String),
    Unknown,
}

enum Ref {
    /// `\1` .. `\9`, `\k<n>`
    Num(usize),
    /// `\k<-n>`: n, and the groups opened before it
    Rel(usize, usize),
    /// `\g<n>`
    CallNum(usize),
    /// `\g<name>`
    CallName(String),
}

#[derive(Default)]
struct Scan {
    c: Vec<char>,
    i: usize,
    /// Capture groups opened so far.
    groups: usize,
    /// Group names defined so far.
    names: Vec<String>,
    refs: Vec<Ref>,
    /// Look-behind nesting depth.
    lookbehind: usize,
    /// Negative look-behind nesting depth: no capture group may open under one.
    neg_lookbehind: usize,
    bad_lookbehind: bool,
}

type R = Result<(), Stop>;

const POSIX: [&str; 14] = [
    "alnum", "alpha", "ascii", "blank", "cntrl", "digit", "graph", "lower", "print", "punct", "space", "upper", "xdigit",
    "word",
];

/// `\p{..}` names Onigmo accepts (folded as it folds them), checked against Ruby 4.0: POSIX
/// classes, general categories (short and long), common binary properties and major scripts.
const PROPERTY_NAMES: &[&str] = &[
    "alnum", "alpha", "ascii", "blank", "cntrl", "digit", "graph", "lower", "print", "punct", "space", "upper", "xdigit",
    "word", "any", "l", "lu", "ll", "lt", "lm", "lo", "lc", "m", "mn", "mc", "me", "n", "nd", "nl", "no", "p", "pc", "pd",
    "ps", "pe", "pi", "pf", "po", "s", "sm", "sc", "sk", "so", "z", "zs", "zl", "zp", "c", "cc", "cf", "cs", "co", "cn",
    "letter", "casedletter", "mark", "number", "punctuation", "symbol", "separator", "other", "alphabetic", "uppercase",
    "lowercase", "whitespace", "emoji", "latin", "latn", "greek", "cyrillic", "arabic", "hebrew", "han", "hiragana",
    "katakana", "hangul", "thai", "common", "inherited", "unknown", "inbasiclatin",
];

fn err<T>(m: &str) -> Result<T, Stop> {
    Err(Stop::Error(m.into()))
}

/// A class item's value: a character, or something that is not one (`\d`, `[:alpha:]`, a set).
#[derive(Clone, Copy, PartialEq)]
enum Item {
    Char(u32),
    Type,
    Posix,
    Set,
    Unknown,
}

impl Scan {
    fn peek(&self, k: usize) -> Option<char> {
        self.c.get(self.i + k).copied()
    }

    fn variable_in_lookbehind(&mut self) {
        if self.lookbehind > 0 {
            self.bad_lookbehind = true;
        }
    }

    /// A capture group opens (`(`, `(?<name>`, `(?'name'`): Onigmo allows none inside a negative
    /// look-behind, at any depth (`(?<!(?:(a)))`, `(?<!(?<=(a)))`).
    fn capture(&mut self) {
        self.groups += 1;
        if self.neg_lookbehind > 0 {
            self.bad_lookbehind = true;
        }
    }

    /// Alternatives up to `)` (when `group`) or the end.
    fn alternation(&mut self, group: bool) -> R {
        while let Some(ch) = self.peek(0) {
            match ch {
                ')' if group => return Ok(()),
                ')' => return Ok(()),
                '|' => self.i += 1,
                '(' => {
                    self.group()?;
                    self.quantifiers()?;
                }
                '[' => {
                    self.i += 1;
                    self.class()?;
                    self.quantifiers()?;
                }
                '\\' => {
                    self.escape()?;
                    self.quantifiers()?;
                }
                '*' | '+' | '?' | '{' => self.quantifiers()?,
                _ => {
                    self.i += 1;
                    self.quantifiers()?;
                }
            }
        }
        if group { Err(Stop::Unknown) } else { Ok(()) }
    }

    /// Quantifiers after an atom (`{` that is not an interval is a literal).
    fn quantifiers(&mut self) -> R {
        loop {
            match self.peek(0) {
                Some('*' | '+' | '?') => {
                    self.variable_in_lookbehind();
                    self.i += 1;
                }
                Some('{') => match self.interval()? {
                    Some(fixed) => {
                        if !fixed {
                            self.variable_in_lookbehind();
                        }
                    }
                    None => {
                        self.i += 1;
                        return Ok(());
                    }
                },
                _ => return Ok(()),
            }
        }
    }

    /// `{n}`, `{n,}`, `{,m}`, `{n,m}` at `self.i`: Some(fixed length) and consumed, or None when
    /// the brace is a literal.
    fn interval(&mut self) -> Result<Option<bool>, Stop> {
        let start = self.i + 1;
        let mut j = start;
        let digits = |c: &[char], j: &mut usize| {
            let s = *j;
            while c.get(*j).is_some_and(|c| c.is_ascii_digit()) {
                *j += 1;
            }
            c[s..*j].iter().collect::<String>()
        };
        let low = digits(&self.c, &mut j);
        let (high, comma) = if self.c.get(j) == Some(&',') {
            j += 1;
            (digits(&self.c, &mut j), true)
        } else {
            (low.clone(), false)
        };
        if self.c.get(j) != Some(&'}') || (low.is_empty() && high.is_empty()) {
            return Ok(None);
        }
        self.i = j + 1;
        // Onigmo's ONIG_MAX_REPEAT_NUM.
        let num = |s: &str| if s.is_empty() { Ok(None) } else { s.parse::<u64>().map(Some).or(Ok(Some(u64::MAX))) };
        let (lo, hi): (Option<u64>, Option<u64>) = (num(&low)?, num(&high)?);
        if lo.is_some_and(|v| v > 100_000) || hi.is_some_and(|v| v > 100_000) {
            return err("too big number for repeat range");
        }
        if let (Some(l), Some(h)) = (lo, hi)
            && l > h
        {
            return err("upper is smaller than lower in repeat range");
        }
        Ok(Some(!comma))
    }

    fn group(&mut self) -> R {
        self.i += 1; // (
        let mut lookbehind = false;
        let negative = self.peek(2) == Some('!');
        if self.peek(0) == Some('?') {
            match (self.peek(1), self.peek(2)) {
                (Some('#'), _) => {
                    while let Some(c) = self.peek(0) {
                        self.i += 1;
                        if c == ')' {
                            return Ok(());
                        }
                    }
                    return Err(Stop::Unknown);
                }
                (Some('<'), Some('=' | '!')) => {
                    self.i += 3;
                    lookbehind = true;
                }
                (Some('<'), _) | (Some('\''), _) => {
                    let close = if self.peek(1) == Some('<') { '>' } else { '\'' };
                    self.i += 2;
                    let name = self.name(close)?;
                    if name.is_empty() {
                        return err("group name is empty");
                    }
                    if !name.chars().next().is_some_and(|c| c.is_alphabetic() || c == '_') {
                        return Err(Stop::Unknown);
                    }
                    self.capture();
                    self.names.push(name);
                }
                (Some(':'), _) => self.i += 2,
                (Some('>' | '=' | '!' | '~'), _) => {
                    self.variable_in_lookbehind();
                    self.i += 2;
                }
                _ => {
                    // Options: `(?imx-imx)` or `(?imx-imx:`; extended mode is not modelled.
                    let mut j = self.i + 1;
                    while self.c.get(j).is_some_and(|c| matches!(c, 'i' | 'm' | '-')) {
                        j += 1;
                    }
                    match self.c.get(j) {
                        Some(')') => {
                            self.i = j + 1;
                            return Ok(());
                        }
                        Some(':') => self.i = j + 1,
                        _ => return Err(Stop::Unknown),
                    }
                }
            }
        } else {
            self.capture();
        }
        if lookbehind {
            self.lookbehind += 1;
            self.neg_lookbehind += usize::from(negative);
        }
        self.alternation(true)?;
        if lookbehind {
            self.lookbehind -= 1;
            self.neg_lookbehind -= usize::from(negative);
        }
        self.i += 1; // )
        Ok(())
    }

    /// A group name up to `close` (consumed).
    fn name(&mut self, close: char) -> Result<String, Stop> {
        let mut name = String::new();
        loop {
            match self.peek(0) {
                Some(c) if c == close => {
                    self.i += 1;
                    return Ok(name);
                }
                Some(c) if c.is_alphanumeric() || c == '_' || c == '-' || c == '+' => {
                    name.push(c);
                    self.i += 1;
                }
                _ => return Err(Stop::Unknown),
            }
        }
    }

    fn escape(&mut self) -> R {
        let Some(c) = self.peek(1) else { return Err(Stop::Unknown) };
        match c {
            'k' | 'g' if matches!(self.peek(2), Some('<' | '\'')) => {
                let close = if self.peek(2) == Some('<') { '>' } else { '\'' };
                self.i += 3;
                let name = self.name(close)?;
                self.reference(c == 'k', &name)
            }
            '1'..='9' => {
                if self.peek(2).is_some_and(|d| d.is_ascii_digit()) {
                    return Err(Stop::Unknown);
                }
                self.i += 2;
                self.variable_in_lookbehind();
                self.refs.push(Ref::Num(c as usize - '0' as usize));
                Ok(())
            }
            'Z' | 'z' | 'R' | 'X' => {
                self.i += 2;
                self.variable_in_lookbehind();
                Ok(())
            }
            'x' | 'c' | 'C' | 'M' | 'p' | 'P' | 'u' | 'o' => self.class_escape().map(|_| ()),
            _ => {
                self.i += 2;
                Ok(())
            }
        }
    }

    /// `\k<..>` (backref) or `\g<..>` (call) with its name.
    fn reference(&mut self, backref: bool, name: &str) -> R {
        if name.is_empty() {
            return err("group name is empty");
        }
        if backref {
            self.variable_in_lookbehind();
            // A backref has no relative-forward form: `\k<+1>` is looked up as a name.
            if name.starts_with('+') {
                return err(&format!("undefined name <{name}> reference"));
            }
        }
        let first = name.chars().next().unwrap();
        if first.is_ascii_digit() || ((first == '-' || first == '+') && name.len() > 1) {
            let Ok(n) = name.parse::<i64>() else { return Err(Stop::Unknown) };
            match (backref, n) {
                (_, 0) => return Err(Stop::Unknown),
                (true, n) if n > 0 && !name.starts_with('+') => self.refs.push(Ref::Num(n as usize)),
                (true, n) if n < 0 => self.refs.push(Ref::Rel((-n) as usize, self.groups)),
                (false, n) if n > 0 && !name.starts_with('+') => self.refs.push(Ref::CallNum(n as usize)),
                _ => return Err(Stop::Unknown),
            }
            return Ok(());
        }
        // `\k<name+level>` / `\k<name-level>`
        let base = match name.find(['+', '-']) {
            Some(k) if backref && name[k + 1..].chars().all(|c| c.is_ascii_digit()) && k + 1 < name.len() => &name[..k],
            Some(_) => return Err(Stop::Unknown),
            None => name,
        };
        if !first.is_alphabetic() && first != '_' {
            return Err(Stop::Unknown);
        }
        if backref {
            if !self.names.iter().any(|n| n == base) {
                return err(&format!("undefined name <{base}> reference"));
            }
        } else {
            self.refs.push(Ref::CallName(base.to_string()));
        }
        Ok(())
    }

    /// An escape that stands for one character (or a character type) at `self.i`, consumed.
    fn class_escape(&mut self) -> Result<Item, Stop> {
        let Some(c) = self.peek(1) else { return Err(Stop::Unknown) };
        let hex = |s: &Scan, from: usize, max: usize| {
            let mut j = from;
            while j < from + max && s.c.get(j).is_some_and(|c| c.is_ascii_hexdigit()) {
                j += 1;
            }
            (j, s.c[from..j].iter().collect::<String>())
        };
        let item = match c {
            'd' | 'D' | 'w' | 'W' | 's' | 'S' | 'h' | 'H' => {
                self.i += 2;
                Item::Type
            }
            'p' | 'P' => {
                if self.peek(2) != Some('{') {
                    return Err(Stop::Unknown);
                }
                let mut j = self.i + 3;
                while self.c.get(j).is_some_and(|c| *c != '}') {
                    j += 1;
                }
                if j >= self.c.len() {
                    return Err(Stop::Unknown);
                }
                // `\p{^Name}` negates; the message drops the `^`. Onigmo looks the name up
                // lowercased without spaces, hyphens and underscores.
                let name: String = self.c[self.i + 3..j].iter().collect();
                let name = name.strip_prefix('^').unwrap_or(&name);
                let folded: String = name.chars().filter(|c| !matches!(c, ' ' | '-' | '_')).flat_map(char::to_lowercase).collect();
                if name.is_empty() {
                    return err("invalid character property name {}");
                }
                // ponytail: only common names are known; Onigmo's full table (scripts, blocks,
                // ages, derived properties) is not modelled, so any other name stops the check.
                if !PROPERTY_NAMES.contains(&folded.as_str()) {
                    return Err(Stop::Unknown);
                }
                self.i = j + 1;
                Item::Type
            }
            'x' => {
                let (j, h) = hex(self, self.i + 2, 2);
                if h.is_empty() {
                    return Err(Stop::Unknown);
                }
                self.i = j;
                Item::Char(u32::from_str_radix(&h, 16).unwrap())
            }
            'u' if self.peek(2) == Some('{') => {
                let (j, h) = hex(self, self.i + 3, 8);
                if h.is_empty() || self.c.get(j) != Some(&'}') {
                    return Err(Stop::Unknown);
                }
                self.i = j + 1;
                Item::Char(u32::from_str_radix(&h, 16).unwrap())
            }
            'u' => {
                let (j, h) = hex(self, self.i + 2, 4);
                if h.len() != 4 {
                    return Err(Stop::Unknown);
                }
                self.i = j;
                Item::Char(u32::from_str_radix(&h, 16).unwrap())
            }
            '0'..='7' => {
                let mut j = self.i + 1;
                while j < self.i + 4 && self.c.get(j).is_some_and(|c| ('0'..='7').contains(c)) {
                    j += 1;
                }
                let o: String = self.c[self.i + 1..j].iter().collect();
                self.i = j;
                Item::Char(u32::from_str_radix(&o, 8).unwrap())
            }
            'c' | 'C' | 'M' | 'o' => return Err(Stop::Unknown),
            _ => {
                self.i += 2;
                match c {
                    'n' => Item::Char(0x0a),
                    't' => Item::Char(0x09),
                    'r' => Item::Char(0x0d),
                    'f' => Item::Char(0x0c),
                    'v' => Item::Char(0x0b),
                    'a' => Item::Char(0x07),
                    'e' => Item::Char(0x1b),
                    c if c.is_ascii_alphanumeric() => Item::Unknown,
                    c => Item::Char(c as u32),
                }
            }
        };
        Ok(item)
    }

    /// One class item at `self.i` (inside `[...]`).
    fn class_item(&mut self) -> Result<Item, Stop> {
        match self.peek(0) {
            Some('[') if self.peek(1) == Some(':') => {
                // `[:name:]` or `[:^name:]`, else a nested set.
                let mut j = self.i + 2;
                if self.c.get(j) == Some(&'^') {
                    j += 1;
                }
                let s = j;
                while self.c.get(j).is_some_and(|c| c.is_ascii_alphabetic()) {
                    j += 1;
                }
                if self.c.get(j) == Some(&':') && self.c.get(j + 1) == Some(&']') && j > s {
                    let name: String = self.c[s..j].iter().collect();
                    if !POSIX.contains(&name.as_str()) {
                        return err("invalid POSIX bracket type");
                    }
                    self.i = j + 2;
                    return Ok(Item::Posix);
                }
                Err(Stop::Unknown)
            }
            Some('[') => {
                self.i += 1;
                self.class()?;
                Ok(Item::Set)
            }
            Some('\\') => self.class_escape(),
            Some(c) => {
                self.i += 1;
                Ok(Item::Char(c as u32))
            }
            None => Err(Stop::Unknown),
        }
    }

    /// A character class after its `[`, through its `]`.
    fn class(&mut self) -> R {
        if self.peek(0) == Some('^') {
            self.i += 1;
        }
        if self.peek(0) == Some(']') {
            // `[]` / `[]a]`: Prism reports the empty class.
            return Err(Stop::Unknown);
        }
        let mut first = true;
        loop {
            match self.peek(0) {
                None => return Err(Stop::Unknown),
                Some(']') if !first => {
                    self.i += 1;
                    return Ok(());
                }
                Some('&') if self.peek(1) == Some('&') => {
                    self.i += 2;
                    first = true;
                    continue;
                }
                _ => {}
            }
            first = false;
            let start = self.class_item()?;
            // A range: `-` not followed by `]`.
            if self.peek(0) != Some('-') || matches!(self.peek(1), Some(']') | None) {
                continue;
            }
            if self.peek(1) == Some('[') && self.peek(2) != Some(':') {
                // `a-[b]`: a literal `-`, then a set.
                self.i += 1;
                continue;
            }
            if matches!(start, Item::Type | Item::Posix) {
                return err("unmatched range specifier in char-class");
            }
            if start == Item::Set {
                return Err(Stop::Unknown);
            }
            self.i += 1;
            let end = self.class_item()?;
            match (start, end) {
                (_, Item::Type | Item::Posix) => return err("char-class value at end of range"),
                (Item::Char(a), Item::Char(b)) if a > b => return err("empty range in char class"),
                (Item::Char(_), Item::Char(_)) => {}
                _ => return Err(Stop::Unknown),
            }
        }
    }
}

/// Whether Onigmo compiles `source`: `error` and an unbalanced `)` (which Prism reports in a
/// literal) make it false; None when the scanner does not model something in it.
pub fn compiles(source: &str) -> Option<bool> {
    let mut s = Scan { c: source.chars().collect(), i: 0, ..Scan::default() };
    match s.alternation(false) {
        Err(Stop::Unknown) => None,
        Err(Stop::Error(_)) => Some(false),
        Ok(()) => Some(s.i >= s.c.len() && error(source).is_none()),
    }
}

#[cfg(test)]
mod tests {
    use super::error;

    // Expected messages from Ruby 4.0 `Regexp.new(source)` (Onigmo).
    #[test]
    fn onigmo_messages() {
        let cases: &[(&str, Option<&str>)] = &[
            (r"(?<n>a)\k<m>", Some("undefined name <m> reference")),
            (r"(?<n>a)\g<m>", Some("undefined name <m> reference")),
            (r"\k<1>", Some("invalid backref number/name")),
            (r"\1", Some("invalid backref number/name")),
            (r"(a)\2", Some("invalid backref number/name")),
            (r"\2(a)", Some("invalid backref number/name")),
            (r"\1(a)", None),
            (r"(a)\1", None),
            (r"(?<n>a)\1", Some("numbered backref/call is not allowed. (use name)")),
            (r"(?<n>a)\g<1>", Some("numbered backref/call is not allowed. (use name)")),
            (r"(a)\k<-1>", None),
            (r"(a)\k<-2>", Some("invalid backref number/name")),
            (r"(a)\g<2>", Some("undefined group <2> reference")),
            (r"\g<n>(?<n>a)", None),
            (r"\k<n>(?<n>a)", Some("undefined name <n> reference")),
            (r"(?<n>a)\k<n+1>", None),
            (r"\k<n-1>", Some("undefined name <n> reference")),
            (r"(?<n>a)(?<n>b)\k<n>", None),
            (r"(?'n'a)\k'm'", Some("undefined name <m> reference")),
            (r"(?<>a)", Some("group name is empty")),
            (r"\k<>", Some("group name is empty")),
            ("a{3,2}", Some("upper is smaller than lower in repeat range")),
            ("a{03,2}", Some("upper is smaller than lower in repeat range")),
            ("{3,2}", Some("upper is smaller than lower in repeat range")),
            ("a{100001}", Some("too big number for repeat range")),
            ("a{,100001}", Some("too big number for repeat range")),
            ("a{100000}", None),
            ("a{,}", None),
            ("a{3, 2}", None),
            ("a{2,3", None),
            ("(?#a{3,2})", None),
            (r"\1a{3,2}", Some("upper is smaller than lower in repeat range")),
            (r"a{3,2}\k<n>", Some("upper is smaller than lower in repeat range")),
            ("[b-a]", Some("empty range in char class")),
            ("[a-a]", None),
            (r"[\x62-a]", Some("empty range in char class")),
            (r"[\n-\t]", Some("empty range in char class")),
            (r"[\142-a]", Some("empty range in char class")),
            (r"[\u{62}-a]", Some("empty range in char class")),
            (r"[a-\]]", Some("empty range in char class")),
            (r"[\e-\a]", Some("empty range in char class")),
            ("[a&&b-a]", Some("empty range in char class")),
            (r"[a-\d]", Some("char-class value at end of range")),
            (r"[\d-a]", Some("unmatched range specifier in char-class")),
            (r"[\d-]", None),
            ("[a-z-0]", None),
            ("[!--]", None),
            ("[--a]", None),
            ("[a-[b]]", None),
            (r"[\[-a]", None),
            ("[[:foo:]]", Some("invalid POSIX bracket type")),
            ("[[:^foo:]]", Some("invalid POSIX bracket type")),
            ("[[:alpha:]]", None),
            ("[:foo:]", None),
            ("[[:alpha:]-a]", Some("unmatched range specifier in char-class")),
            ("[a-[:alpha:]]", Some("char-class value at end of range")),
            ("(?<=a+)b", Some("invalid pattern in look-behind")),
            ("(?<=a?)b", Some("invalid pattern in look-behind")),
            ("(?<=a{1,2})b", Some("invalid pattern in look-behind")),
            ("(?<=a{2})b", None),
            ("(?<=a|bc)b", None),
            ("(?<=a|b+)", Some("invalid pattern in look-behind")),
            ("(?<=a(?=b))", Some("invalid pattern in look-behind")),
            ("(?<=(?>a))", Some("invalid pattern in look-behind")),
            (r"(a)(?<=\1)", Some("invalid pattern in look-behind")),
            (r"(?<=a\z)", Some("invalid pattern in look-behind")),
            (r"(?<=\d)", None),
            ("(?<=(?i:ab))", None),
            ("(?<=(?<n>a))", None),
            ("(?<=a+)[b-a]", Some("empty range in char class")),
            ("(?<n>a)\\1a{3,2}", Some("upper is smaller than lower in repeat range")),
            ("a{3,2}[b-a]", Some("upper is smaller than lower in repeat range")),
            ("[b-a]a{3,2}", Some("empty range in char class")),
            (r"\p{Alpha}+(?:\.\d+)*", None),
            ("(?<!(a))b", Some("invalid pattern in look-behind")),
            ("(?<!(?<n>a))", Some("invalid pattern in look-behind")),
            ("(?<!a|(b))", Some("invalid pattern in look-behind")),
            ("(?<!(?:(a)))", Some("invalid pattern in look-behind")),
            ("(?<!(?<=(a)))", Some("invalid pattern in look-behind")),
            ("(?<=(?<!(a)))", Some("invalid pattern in look-behind")),
            ("(?<!(?:a))", None),
            ("(?<!(?i:a))", None),
            ("(?<!a)(b)", None),
            ("(?<!a|bc)", None),
            (r"(a)\k<+1>", Some("undefined name <+1> reference")),
            (r"(a)\k<+0>", Some("undefined name <+0> reference")),
            (r"\k<+1>(?<n>a)", Some("undefined name <+1> reference")),
            (r"(a)\k<+1>\k<m>", Some("undefined name <+1> reference")),
            (r"(a)\k<m>\k<+1>", Some("undefined name <m> reference")),
            (r"(a)\k<-1>\k<+1>", Some("undefined name <+1> reference")),
            (r"\p{Alpha}[b-a]", Some("empty range in char class")),
            (r"\p{ L u }[b-a]", Some("empty range in char class")),
            (r"\p{^Alpha}[b-a]", Some("empty range in char class")),
            (r"\P{In_Basic_Latin}[b-a]", Some("empty range in char class")),
            (r"[\p{Lu}-a]", Some("unmatched range specifier in char-class")),
            (r"\p{}", Some("invalid character property name {}")),
            (r"\p{^}", Some("invalid character property name {}")),
            // Unknown names stop the check (Onigmo: invalid character property name {Foo}).
            (r"\p{Foo}[b-a]", None),
            (r"[\p{Foo}-a]", None),
            (r"\A[\w.-]+@[a-z\d-]+\z", None),
            (r"^(\d+)-(\d+)$", None),
        ];
        for (src, want) in cases {
            assert_eq!(error(src).as_deref(), *want, "{src}");
        }
    }
}
